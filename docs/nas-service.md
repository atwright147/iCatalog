# Run iCatalog as a Linux service

The supplied [systemd unit](../packaging/systemd/icatalog.service) runs the static
NAS CLI as a dedicated, unprivileged `icatalog` account. It serves an existing
authoritative catalogue at `/var/lib/icatalog/catalogue.icatalog`, requires a
token file, and listens on `127.0.0.1:8080`. A separate
[HTTPS drop-in](../packaging/systemd/https.conf) enables direct remote access.
Each client keeps a local catalogue and connects through the sync API.

These instructions target Linux hosts using systemd and standard `useradd`,
`install` and `sudo` tools. Vendor NAS firmware may use another service manager;
use its supported process supervision or [Docker/Compose](nas-docker.md) there.
Installation is explicit: the package does not create users, initialize a
catalogue, enable a service, or change firewall rules automatically.

## Install the binary and create the service account

Download the CLI-only archive matching the NAS CPU (`x86_64-unknown-linux-musl`
or `aarch64-unknown-linux-musl`) and verify it against its release `SHA256SUMS`.
If building locally, use the [native NAS tasks](development.md#8a-static-nas-cli-builds-d58).
Extract into a new directory, then run `./icatalog --version` there. Run the
following as a NAS administrator, after checking that neither the service account
nor the destination paths are already used by another installation:

```sh
sudo useradd --system --user-group --home-dir /var/lib/icatalog --no-create-home --shell /usr/sbin/nologin icatalog
sudo install -o root -g root -m 0755 ./icatalog /usr/local/bin/icatalog
sudo install -d -o icatalog -g icatalog -m 0700 /var/lib/icatalog
sudo install -d -o root -g icatalog -m 0750 /etc/icatalog
```

Reuse a suitable existing service account instead of recreating it. Keep the
binary root-owned so the service cannot replace its executable. The whole
catalogue directory must be writable, including SQLite rollback journals,
snapshot files and `previews/<sha>.jpg`; granting access to just the SQLite file
is insufficient. Do not grant write permission to the original media library.

Prepare a random token in a protected file using the
[secure sync instructions](sync-user-guide.md#secure-server-and-clients), then
install it without putting its contents on the command line:

```sh
sudo install -o root -g icatalog -m 0640 /secure/staging/sync.token /etc/icatalog/sync.token
```

The token and later TLS key are outside the catalogue and preview directory.
Clients receive their token through a separate secure channel. File paths appear
in service configuration; token contents do not.

## Prepare the catalogue

For a new catalogue, explicitly initialize it and import originals from their
stable absolute path:

```sh
sudo -u icatalog /usr/local/bin/icatalog init /var/lib/icatalog/catalogue.icatalog
sudo -u icatalog /usr/local/bin/icatalog import /var/lib/icatalog/catalogue.icatalog /srv/photos
sudo -u icatalog /usr/local/bin/icatalog previews /var/lib/icatalog/catalogue.icatalog external
sudo -u icatalog /usr/local/bin/icatalog previews /var/lib/icatalog/catalogue.icatalog generate
```

Preview generation is optional. Grant the service account read/traverse access
to `/srv/photos` for these maintenance commands using the NAS's existing group or
ACL policy; do not recursively change the media library's ownership. The running
sync service serves stored catalogue data and previews without reading originals.

For an existing authoritative catalogue, stop all its writers and preserve a
backup before copying the catalogue and its entire external `previews` directory
into `/var/lib/icatalog`. Set ownership of these copied files to `icatalog`.
Do not initialize over that copy or use an established client replica as the
server. Stored media paths remain unchanged. Validate the prepared catalogue:

```sh
sudo -u icatalog /usr/local/bin/icatalog open /var/lib/icatalog/catalogue.icatalog
```

## Install and start the service

From a matching source checkout, install the reviewed unit:

```sh
sudo install -o root -g root -m 0644 packaging/systemd/icatalog.service /etc/systemd/system/icatalog.service
sudo systemd-analyze verify /etc/systemd/system/icatalog.service
sudo systemctl daemon-reload
sudo systemctl enable --now icatalog.service
sudo systemctl status icatalog.service
sudo journalctl -u icatalog.service -n 100 --no-pager
```

Startup fails if the catalogue, token or configured security files are absent or
invalid. It does not create an empty catalogue. The unit limits automatic retries
to three starts within a minute; after correcting a repeated startup failure,
run `sudo systemctl reset-failed icatalog.service`, then start it again.

The default listener is suitable for same-host clients or a TLS reverse proxy.
A local CLI client can connect with:

```sh
icatalog clone local.icatalog --url http://127.0.0.1:8080 --token-file /secure/client/sync.token
```

HTTP with a token is allowed only on literal loopback addresses. Do not change
the bind to a LAN address without enabling HTTPS. A reverse proxy must terminate
TLS, forward the Authorization header, and leave the upstream listener on
loopback. See [sync setup and recovery](sync-user-guide.md).

## Enable direct HTTPS access

Obtain a certificate whose names match the hostname clients will use. Install
its PEM chain and unencrypted PEM private key as protected files:

```sh
sudo install -o root -g icatalog -m 0640 /secure/staging/server-chain.pem /etc/icatalog/server-chain.pem
sudo install -o root -g icatalog -m 0640 /secure/staging/server-key.pem /etc/icatalog/server-key.pem
sudo install -d -o root -g root -m 0755 /etc/systemd/system/icatalog.service.d
sudo install -o root -g root -m 0644 packaging/systemd/https.conf /etc/systemd/system/icatalog.service.d/https.conf
sudo systemd-analyze verify /etc/systemd/system/icatalog.service
sudo systemctl daemon-reload
sudo systemctl restart icatalog.service
```

Review the drop-in before installation: its listener is `0.0.0.0:8443`. Choose
the intended interface and allow only the intended clients through the NAS
firewall. The empty `ExecStart=` resets the base command before the HTTPS command
is assigned. The existing account, writable-directory restriction and other
service protections remain active.

Clients use `https://<certificate-host>:8443`, their token file, and `--ca-file`
for a private CA. Desktop Connect/Sync offers matching Security file pickers.
Renew certificates and rotate tokens explicitly, then restart the service;
automatic renewal and per-user accounts are not provided.

If using different catalogue paths, update `WorkingDirectory`, `ReadWritePaths`,
`RequiresMountsFor` and both command definitions together. With
`ProtectSystem=strict`, the catalogue directory is the only persistent writable
path; the private temporary directory remains available for temporary files.
`ProtectHome=yes` makes home directories inaccessible to the service. Keep
catalogues and security files under the service directories shown above.
Systemd documents these [execution protections](https://www.freedesktop.org/software/systemd/man/latest/systemd.exec.html).

## Maintain, upgrade and remove

Stop the service before importing, updating, moving files or taking a backup.
Run maintenance commands as `icatalog`, then start the service again:

```sh
sudo systemctl stop icatalog.service
sudo -u icatalog /usr/local/bin/icatalog update /var/lib/icatalog/catalogue.icatalog
sudo -u icatalog /usr/local/bin/icatalog previews /var/lib/icatalog/catalogue.icatalog generate
sudo systemctl start icatalog.service
```

Systemd sends SIGTERM to the process group and bounds stopping to 30 seconds.
The process does not drain in-flight requests; clients may need to retry. SQLite
recovers interrupted transactions when reopened. Committed catalogue data and
external previews remain in `/var/lib/icatalog`. Restart-on-failure does not
restart a service explicitly stopped through `systemctl`.
See systemd's [service lifecycle](https://www.freedesktop.org/software/systemd/man/latest/systemd.service.html).

For an upgrade, first verify the new archive and record the current CLI version.
Stop the service and every other writer, back up the entire catalogue directory
and protected configuration, and retain the old binary. Install the new binary
as root, validate the catalogue with `open` as `icatalog`, then start the service
and check its logs and a client connection. Review changed unit files before
replacing them; run `daemon-reload` after unit changes. Do not run an older binary
against a catalogue migrated by a newer release: a rollback requires the matching
pre-upgrade catalogue backup as well as the old binary. Preserve newer data
separately before restoring any backup.

To stop service management without deleting catalogues or credentials:

```sh
sudo systemctl disable --now icatalog.service
sudo systemctl reset-failed icatalog.service
sudo rm /etc/systemd/system/icatalog.service
# Only if the supplied HTTPS drop-in was installed:
sudo rm /etc/systemd/system/icatalog.service.d/https.conf
sudo systemctl daemon-reload
```

Retain `/var/lib/icatalog`, `/etc/icatalog`, the service account and binary until
you have reviewed backups and any other users of them. No removal step above
deletes catalogue data, previews or media.

## Verification scope

The CLI integration suite exercises the shipped command arguments against
disposable catalogues: missing-catalogue refusal, authentication, synchronization
and persistence after process restart, plus malformed TLS settings. It also
checks the unit's account, filesystem and stop/restart policy. These tests do not
boot systemd; run `systemd-analyze verify` and a start/stop/reboot check on the
target Linux host before relying on unattended operation.

`packaging/systemd/test.sh` provides a separate lifecycle check for a disposable
native Linux host already running systemd. Run it as root with the built native
CLI path. It creates a uniquely named test account and runtime unit, substitutes
only its fixture paths, identity and port into the shipped unit, and retains the
service sandbox. It checks missing-catalogue refusal, authenticated sync, clean service
stopping and persistence after restart, then removes its test unit, account and
data. It refuses existing names and does not install or modify `icatalog.service`.
Use a disposable runner; this test does not reboot the machine.
