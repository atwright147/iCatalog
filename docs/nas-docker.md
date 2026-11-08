# Run iCatalog on a NAS with Docker

The container runs the existing `icatalog` CLI and HTTP sync service. Each desktop
keeps its own local catalogue and connects through **File → Connect to Server…**
or `icatalog clone`. Clients never open the NAS SQLite file over a network share.

Images can be built for Linux x86-64 (`linux/amd64`) and ARM64 (`linux/arm64`).
There is no published container registry image yet; build locally or load an image
built for your NAS architecture. TLS and authentication remain unbuilt. The Compose
example publishes to loopback by default; use a trusted LAN address when enabling
access from other computers.

## Build the image

On native Linux of the NAS's architecture, install the [NAS build prerequisites](development.md#8a-static-nas-cli-builds-d58)
and Docker Engine with the Compose plugin. From the repository root, for ARM64:

```sh
task nas:build -- aarch64-unknown-linux-musl
task docker:build -- aarch64-unknown-linux-musl icatalog:local
task docker:test -- icatalog:local
```

Use `x86_64-unknown-linux-musl` on x86-64. These commands reuse the locked static
CLI build and verify its ELF architecture and absence of dynamic dependencies.
The image contains the CLI and empty mount directories, with no shell, compiler
or desktop libraries. Its build context contains only those files; catalogues,
media and the repository are not sent to the image builder.

`ICATALOG_CONTAINER_ENGINE=podman` selects Podman for the build and container tests.
The Compose instructions below use Docker Compose. `CARGO_TARGET_DIR` is honored;
`ICATALOG_DOCKER_BINARY=/absolute/path/to/icatalog` can select an extracted, trusted
NAS archive binary instead of a local build. The ELF checker requires `readelf`
on the machine invoking `docker:build`. Neither task builds through CPU emulation.

To transfer an image from another Linux builder of the same architecture:

```sh
docker save --output icatalog-image.tar icatalog:local
# Copy icatalog-image.tar to the NAS, then run there:
docker load --input icatalog-image.tar
```

## Set paths and ownership

Run the following from the repository root. Replace the example directories with
existing absolute NAS paths. Create a dedicated catalogue directory owned by the
non-root account that will run iCatalog; that account must also be able to read
and traverse the media directory. Do not change ownership of an existing media
library just to match the image's default UID.

```sh
export ICATALOG_CATALOGUE_DIR=/srv/icatalog
export ICATALOG_MEDIA_DIR=/srv/photos
export ICATALOG_UID=$(id -u)
export ICATALOG_GID=$(id -g)
export ICATALOG_IMAGE=icatalog:local
```

The example assumes the current NAS account is non-root. Otherwise set the UID
and GID to the intended service account's numeric values. Create the catalogue
directory with that account, or have the NAS administrator grant it write access.
It needs write access to the directory as well as the SQLite file for journals,
snapshots and external previews. Media is mounted read-only at `/media`.

Compose refuses missing host directories instead of creating them. Store these
variables in your service environment or an environment file and supply it with
`docker compose --env-file /absolute/path/to/settings.env -f packaging/docker/compose.yml …`.
Environment-file values use `NAME=value`, without `export`. Keep the host paths
absolute so they do not depend on Compose's project directory.

## Create and import a catalogue

For a **new** catalogue, explicitly initialize it, then import through the mounted
media path:

```sh
docker compose -f packaging/docker/compose.yml run --rm --no-deps icatalog init /catalogue/catalogue.sqlite
docker compose -f packaging/docker/compose.yml run --rm --no-deps icatalog import /catalogue/catalogue.sqlite --path /media
docker compose -f packaging/docker/compose.yml run --rm --no-deps icatalog previews /catalogue/catalogue.sqlite external
docker compose -f packaging/docker/compose.yml run --rm --no-deps icatalog previews /catalogue/catalogue.sqlite generate
```

The preview steps are optional. The whole catalogue directory is persistent,
including `previews/<sha>.jpg`. Startup never initializes a missing catalogue;
`serve` reports an error without creating a SQLite file. `init` remains explicit.

To use an existing authoritative catalogue, stop all writers, preserve a backup,
and copy the catalogue and its external `previews` directory into the mounted data
directory. The example uses the filename `catalogue.sqlite`; change the Compose
command if keeping another filename. Stored media paths are not rewritten: an
existing catalogue that references `/srv/photos` needs its media bind target kept
at `/srv/photos`, or media reimported into a new catalogue using `/media`. Keep
those targets stable across container updates.

## Start, connect and maintain

Choose the NAS's trusted LAN address for publication, for example:

```sh
export ICATALOG_BIND=192.168.1.20
export ICATALOG_PORT=8080
docker compose -f packaging/docker/compose.yml up -d
docker compose -f packaging/docker/compose.yml logs --tail 100 icatalog
```

Inside the container the listener binds to `0.0.0.0:8080`; `ICATALOG_BIND` controls
the published host interface. Omit it to retain the loopback-only default. Allow
the selected port through the NAS firewall for trusted clients.

On each desktop, connect to `http://192.168.1.20:8080` and choose a new local file.
For CLI use:

```sh
icatalog clone local.sqlite --url http://192.168.1.20:8080
icatalog edit local.sqlite --id 1 --rating 5
icatalog sync local.sqlite --url http://192.168.1.20:8080
```

Use an actual asset ID from `icatalog list local.sqlite`. See the
[sync user guide](sync-user-guide.md) for offline edits and conflict recovery.

For a rescan, stop the service, run the maintenance commands, then restart:

```sh
docker compose -f packaging/docker/compose.yml stop icatalog
docker compose -f packaging/docker/compose.yml run --rm --no-deps icatalog update /catalogue/catalogue.sqlite
docker compose -f packaging/docker/compose.yml run --rm --no-deps icatalog previews /catalogue/catalogue.sqlite generate
docker compose -f packaging/docker/compose.yml up -d
```

Compose injects an init process to forward stop signals. Committed edits and
previews survive stopping, removing and recreating the container; SQLite recovers
interrupted transactions on reopening. In-flight requests may need retrying.
`docker compose … down` removes containers and the project network while retaining
the bind-mounted catalogue and media. Back up the data directory with writers
stopped. To upgrade, stop the service, back up its data, load/build the replacement
image, then use `up -d --force-recreate` with the same mounts.

If using `docker run` directly, pass `--init`, mount the catalogue directory
read/write and media read-only, and select an appropriate `--user UID:GID`.
The image defaults to UID/GID `1000:1000`; it does not chown mounts or elevate
permissions. Its default command is `serve /catalogue/catalogue.sqlite --bind
0.0.0.0 --port 8080`.

## Verification

`task docker:test` uses disposable generated media and catalogues, an isolated
container network and no host-published port by default. With
`ICATALOG_TEST_COMPOSE=1`, the same test also exercises the supplied Docker Compose
service using an ephemeral loopback port and its isolated network. It checks startup failure without
file creation, import, external-preview generation/fetch, read-only mounts,
non-root configuration, clone/edit/Collection sync, prompt signal handling and
fresh-clone persistence after replacing the server container.

Run runtime checks on Linux-local storage visible to the engine. Local validation
passed on native ARM64 Linux and with the x86-64 application under emulation.
Compose v2.40.3 also passed using Podman's Docker-compatible API; a native Docker
backend and GitHub-hosted execution remain unverified. The Mac/libkrun shared
filesystem returned a SQLite I/O error editing a cloned fixture, so those checks
used the VM's native temporary storage instead.

On SELinux hosts, arrange appropriate bind-mount labels for the data and media
directories using your runtime's configuration. The automated Podman fixture
disables label separation only for its disposable test mounts; it retains the
numeric user, dropped capabilities and read-only restrictions. The supplied
Compose service keeps normal host labeling enabled.

The native x86-64/ARM64 `docker.yml` workflow builds and tests images without
publishing to a registry. See the devlog for local validation and hosted-run
status; adding a workflow does not establish that it has run on GitHub.

Implementation references: [Docker USER and exec-form ENTRYPOINT](https://docs.docker.com/reference/dockerfile/),
[Compose bind mounts and init](https://docs.docker.com/reference/compose-file/services/).
