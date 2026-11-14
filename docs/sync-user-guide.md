# Using iCatalog sync

Use sync to share a catalogue between computers while keeping a local copy on each one. One machine—usually your NAS—holds the main catalogue and original media. Other computers browse their local copies and send back ratings, labels, keywords and Collection edits.

Sync is available in the desktop app through **File → Connect to Server…** and **File → Sync…**, as well as the CLI. Opening a catalogue does not sync automatically. When using the CLI to sync, close the desktop app first and reopen it afterward; desktop sync refreshes the open catalogue itself.

## Before you start

Maximum Rating Smart rules require schema-v8 support (D57). Update iCatalog on the server and each client before syncing an upgraded catalogue. Older apps refuse the newer catalogue; an older server rejects a maximum-rule upload and leaves the client draft intact. Minimum and maximum are inclusive, can be used independently, and sync with the Collection’s other rules. A maximum of 0 matches unrated assets.

Use builds containing protocol v2 (D20/D44) on both server and clients, and the new client-side `clone` command (D45) for the recommended setup below. Older builds cannot sync with this protocol. If running from this repository, `task build` creates the CLI and desktop binaries under `target/debug/`; use `task cli:run --` in place of `icatalog`, and `task ui:run --` in place of `icatalog-ui` in the examples below. Installed binaries are assumed to be on your PATH.

For a Linux NAS, D58 adds CLI-only static musl archives for x86-64 and ARM64. Build/download the archive for your CPU, extract `icatalog` and place it on PATH; no system SQLite or desktop libraries are required. Build commands and CI artifact locations are in [the NAS build guide](development.md#8a-static-nas-cli-builds-d58). The new release jobs have not yet been verified on hosted CI or published NAS assets.

For Docker on a NAS, use the [Docker/Compose setup guide](nas-docker.md) (D60).
It covers image building/loading, non-root permissions, persistent catalogue and
preview storage, read-only media, explicit initialization/import and maintenance.
Images are built locally; no registry image is published yet.

The examples use macOS/Linux shell paths. Replace the catalogue paths, media path and `nas` hostname with your own. On Windows, use the corresponding local paths and installed executables.

Use `.icatalog` for new files. Existing `.sqlite` catalogues work as before;
neither extension is trusted without checking the file contents. Sync backups
keep the original catalogue's extension.

Keep catalogues in a data folder outside the build directory. For a local repository test, `sync-demo/server.icatalog` is suitable; `target/` is build output and is removed by `task clean`.

The service supports HTTPS and shared-token authentication (D98). Use the secure setup below when protecting access to your catalogue. The simple HTTP examples remain available for trusted local networks; they provide no encryption or authentication. `0.0.0.0` listens on every IPv4 interface; you can instead bind to the server's specific LAN address.

## 1. Prepare the main catalogue on the server

For a new shared catalogue, run these commands on the machine that can read the original media. Create a new catalogue filename in a directory you can write to:

```sh
mkdir -p "$HOME/iCatalog"
icatalog init "$HOME/iCatalog/Library.icatalog"
icatalog import "$HOME/iCatalog/Library.icatalog" --path "/path/to/photos"
```

Replace `/path/to/photos` with the actual media directory on that machine. Import scans its subfolders too.

If you already have the main catalogue, use its existing file instead of creating another one. Its stored media paths must be accessible on the server for Import, Update and preview generation; moving the catalogue alone does not rewrite those paths.

Optionally, prepare larger previews on the server:

```sh
icatalog previews "$HOME/iCatalog/Library.icatalog" external
icatalog previews "$HOME/iCatalog/Library.icatalog" generate
```

External storage keeps these previews in a `previews` directory beside the catalogue, so clients can download them individually. Small grid thumbnails remain inside SQLite. Preview generation is optional for normal desktop browsing, which currently displays those small thumbnails.

## 2. Start the service on the server

Run this on the server and leave that terminal open:

```sh
icatalog serve "$HOME/iCatalog/Library.icatalog" --bind 0.0.0.0 --port 8080
```

Clients will use `http://nas:8080`, replacing `nas` with the server's hostname or LAN IP address. If you omit `--bind`, the default is `127.0.0.1`, which accepts connections only from the server itself. Allow the chosen port through the server's firewall for trusted LAN clients.

The service runs until stopped with Ctrl-C. `serve` requires an existing catalogue
file and never creates a missing file; check the configured path/mount or initialize
a new catalogue explicitly. Docker/Compose packaging is available through the
[NAS Docker guide](nas-docker.md); a systemd installation recipe remains unbuilt.

### Secure server and clients

Obtain a PEM server certificate chain and an unencrypted PEM private key for the
hostname clients will use. A public CA certificate works with the bundled roots;
for a private CA, copy its public CA certificate to each client. The certificate
must cover the hostname or IP in the URL. iCatalog does not issue or renew
certificates automatically.

Create a random token on the server, for example with OpenSSL:

```sh
(umask 077; openssl rand -hex 32 > "$HOME/iCatalog/sync.token")
```

Keep the token and server private key readable only by the service account.
Transfer the token privately to each authorised client. Everyone holding this
token can read the catalogue and submit supported edits. Start the server with:

```sh
icatalog serve "$HOME/iCatalog/Library.icatalog" --bind 0.0.0.0 --port 8443 \
  --tls-cert /path/to/server-chain.pem --tls-key /path/to/server-key.pem \
  --token-file "$HOME/iCatalog/sync.token"
```

For the CLI, supply the client token file on each clone, sync or preview fetch:

```sh
icatalog clone "$HOME/Pictures/iCatalog/Library.icatalog" --url https://nas:8443 \
  --token-file /path/to/client.token --ca-file /path/to/private-ca.pem
icatalog sync "$HOME/Pictures/iCatalog/Library.icatalog" --url https://nas:8443 \
  --token-file /path/to/client.token --ca-file /path/to/private-ca.pem
```

Omit `--ca-file` when the certificate is trusted by the bundled public roots.
Do not put tokens in URLs. There is no option to disable certificate validation.
A token file contains 32–4096 ASCII token characters with an optional final
newline; the example above produces a suitable random hexadecimal token.

In **Connect to Server** or **Sync catalogue**, enter the HTTPS address and expand
**Security settings** to choose the token file and optional CA file. Selections
remain while that dialog is open, including after a failed attempt, and clear
when it closes. Choose them again for the next dialog. Only the server address
is remembered in the local sync preferences; credentials and credential-file
paths are remembered only when the option is enabled.

Rotate a token by replacing the server file, restarting the service, and securely
replacing clients' copies. Restart after renewing the certificate/key as well.
No catalogue migration or re-clone is needed. Authentication or certificate
errors leave local work intact; correct the credentials or trust configuration
and retry. These errors differ from an offline `sync deferred` result.

For a TLS reverse proxy on the same host, `serve --bind 127.0.0.1 --port 8080
--token-file ...` allows authenticated loopback HTTP. Forward Authorization to
iCatalog and expose only the proxy's HTTPS endpoint. Remote token use requires
HTTPS; a hostname such as `localhost` is not a literal loopback address.

## 3. Clone a local catalogue on each client

In the desktop app:

1. Choose **File → Connect to Server…**.
2. Enter the **Server address**, for example `http://nas:8080` (or `http://127.0.0.1:8080` when testing on the same computer).
3. Click **Choose local file…** and select a new `.icatalog` filename in an existing local folder.
4. Click **Clone**. The app shows progress, downloads and verifies the catalogue, then opens the new replica. Click **Close** to browse and edit it.

The catalogue you previously had open is preserved. Existing destinations are refused even if the native save picker offers to replace them. Errors keep your address and chosen path so you can correct the problem and retry. A successful clone remembers the server address for this local catalogue.

For the CLI, create the parent directory and choose a **new** local filename:

```sh
mkdir -p "$HOME/Pictures/iCatalog"
icatalog clone "$HOME/Pictures/iCatalog/Library.icatalog" --url http://nas:8080
```

From this repository, the equivalent is `task cli:clone -- local.icatalog --url http://nas:8080`.

Clone downloads a consistent copy while the server remains running, verifies it, preserves its catalogue identity and records the starting point for your future edits. It reports the catalogue ID, generation and download size. The resulting local replica is ready to use immediately: **do not run `init` or copy a file first, and no separate first sync is required.** Your first edits after cloning will be uploaded by your next ordinary sync.

The destination directory must already exist. Clone refuses an existing destination, including an empty file, symlink or existing SQLite sidecar, and rejects the bare `:memory:` path before connecting. Choose another filename if you already have local work; use `sync` to update an established replica. A failed download or validation leaves no new destination file, so you can correct the reported problem and retry. Clone does not upload anything or modify the server catalogue. An unreachable server makes clone fail rather than reporting a successful deferred sync.

Open the cloned catalogue:

```sh
icatalog-ui "$HOME/Pictures/iCatalog/Library.icatalog"
```

Repeat this section for each additional client. Each computer uses its own local SQLite file; clients do not open the main file over SMB/NFS. With an external preview store, the download includes catalogue data and small thumbnails; larger previews can be fetched later. Embedded preview bytes travel inside the catalogue.

CLI commands still require `--url` each time. Desktop connection settings are local to that catalogue filename and are not sent to the server.

## 4. Alternative: copy an existing server catalogue, then sync

You can still seed a client by copying the main `Library.icatalog` using your usual file manager or file-transfer tool. Close all writers, including the server service, before making an ordinary file copy; restart the service afterward. Copy to a separate local filename, not over a client with unsynced edits. Do not create an unrelated empty catalogue with `init`: sync requires the server's catalogue identity.

Before editing a manually copied catalogue, open it in the desktop, choose **File → Sync…**, enter the address, and acknowledge the first-sync message before clicking **Sync now**. The acknowledgement is required because unknown local edits are backed up rather than uploaded. Alternatively, close the desktop app and run:

```sh
icatalog sync "$HOME/Pictures/iCatalog/Library.icatalog" --url http://nas:8080
```

This first sync establishes the server's current state and saves the previous local file in a timestamped backup, for example `Library.rejected-20261031T120000Z.icatalog`. **Existing edits made before this first sync stay in that backup; they are not uploaded automatically.** This also applies when upgrading a client previously synced with the older protocol. Review and reapply wanted edits using the recovery steps below. In the desktop, an asset remains selected only if its stable identity is present in the downloaded catalogue; a reused row number cannot select a different asset (D87). After server state is installed, a folder or device view returns to All because its previous location may not identify the same place on the server. An offline attempt keeps the current view.

The `rejected` filename is also used for backups made during successful syncs; it does not by itself mean edits were rejected. Keep backups until you have checked the result.

This backup-first setup applies to manual copies. A catalogue created by `clone` already has its starting point recorded and can be edited straight away.

## Everyday use

1. Open your local catalogue and choose **File → Sync… → Sync now** to fetch the latest catalogue. The server address is filled in after your first desktop connection or sync attempt; CLI-created replicas need the address entered once.
2. Close the results dialog. Browse, rate, label, add/remove keywords, and create or edit manual and Smart Collections.
3. Choose **File → Sync… → Sync now** again to send your edits and receive the latest server state. The current view and inspector refresh automatically.

The dialog shows the command result, backup paths and any conflict keys in selectable, scrollable text. An offline result says `sync deferred`; it does not mean your edits reached the server. Wait for an operation to finish before closing the app. There is no cancellation during transfer or automatic startup/background sync.

For CLI use, close the app, run `icatalog sync … --url …`, then reopen it. Supply `--url` on every CLI invocation. Sync is explicit, so other computers see your changes after they run their own sync.

The desktop remembers the URL and options in a versioned, catalogue-ID-bound `<catalogue>.sync.toml` file beside the local catalogue, using bounded, atomic replacement. **Remember token and CA file paths on this computer** opts in to saving absolute token/CA paths; token contents are never stored. The required-sync marker comes from the recovery journal; choosing to browse read-only is transient rather than a saved preference. The settings file is not part of the synced database. Existing `.sync-url` files migrate with check-on-open and security-path remembering disabled. If saving settings fails, the dialog reports it separately; the completed sync still stands. Moving/renaming a catalogue requires moving its settings too. A malformed or wrong-catalogue settings file is reported rather than applied.

### Optional check when opening

In File → Sync…, enable **Check for updates when opening** to check this local
catalogue against its saved server when you next open it. The check runs for a
tracked replica after checking local recovery state; an untracked catalogue
skips it. The check does not replace the catalogue. If it finds server changes
or pending local edits, choose whether to sync now or continue browsing
read-only. Saved conflicts or an interrupted replacement instead open a
read-only recovery banner with a **Resolve conflicts** or recovery action.
Read-only mode disables catalogue edits but leaves sync and recovery available.
When sync is not required, **Edit this copy** in the status area resumes local
editing. A required-sync marker cannot be cleared this way.

Security file paths are remembered only when **Remember security file paths**
is enabled in the dialog; the token contents are never stored. If this option
is off, the opening check uses only the saved URL. For an authenticated or
private-CA server, enable path remembering and select the relevant files, or
open File → Sync… and select them in that dialog.

If the server is unreachable before a required sync has succeeded, the copy
stays read-only until you can sync. If no sync was already required, an
unreachable check reports the problem and you can keep working offline. A
successful conflict-free sync clears the required marker.

### Resume an interrupted sync

Opening iCatalog checks the local recovery journal but does not retry a
transfer or install a staged snapshot automatically. The desktop shows the
recovery phase and offers the action that matches it: **Resume interrupted
sync** for `snapshot_ready`, **Sync now** for `retry` or `applied`, and **Apply
saved choices and sync** for `applying`.

For CLI use, inspect the phase first:

```sh
icatalog sync-resolve Library.icatalog --action list
```

For `snapshot_ready`, explicitly verify and resume the staged local replacement:

```sh
icatalog sync-resolve Library.icatalog --action recover
```

This checks the staged digest, catalogue identity/generation and preserved
backup, then installs the staged snapshot (or confirms that it was already
installed). It does not contact the server or upload edits. Reopen the catalogue
in iCatalog after recovery. For `retry` or `applied`, run an ordinary `sync`
with its server URL and any required security files. For `applying`, repeat
`sync-resolve --action apply --session-id <session> --url <server>` with the
session ID returned by `list`. These actions are explicit; opening the
catalogue and read-only checks never restart the transfer.

If the local catalogue disappears or is empty while desktop sync is running,
reopening reports an error and leaves the file absent or empty. Restore the
catalogue from a backup, then use **Reopen catalogue**; recovery does not
initialise a replacement file (D75).

| Change | Where to make it |
|---|---|
| Rating, label, keywords | Any established client copy or the server, including photos with captions or credits; those text fields are preserved |
| Collection creation, rename, Smart rules, manual membership | Any established client copy or the server |
| Import new media, rescan files, refresh extracted metadata | Server |
| Generate larger previews or change their storage mode | Server |

Import and Update are disabled on client copies. Original media is not copied by sync. Collection deletion and conversion between manual and Smart Collections are not supported.

You can also make user edits through the CLI. First inspect your local asset and Collection IDs:

```sh
icatalog list "$HOME/Pictures/iCatalog/Library.icatalog"
icatalog collection "$HOME/Pictures/iCatalog/Library.icatalog" list
```

For example, if the chosen asset has ID `3`:

```sh
icatalog edit "$HOME/Pictures/iCatalog/Library.icatalog" --id 3 --rating 5 --keyword Favourite
icatalog collection "$HOME/Pictures/iCatalog/Library.icatalog" create --name "To print"
icatalog sync "$HOME/Pictures/iCatalog/Library.icatalog" --url http://nas:8080
```

Use the IDs from the current local catalogue, including when editing Collection membership. Full CLI examples are in the [development guide](development.md#4-driving-the-cli).

## Read the sync result

| Output | Meaning and next action |
|---|---|
| `cloned catalogue …` | New local replica created and ready to edit. Subsequent updates use `sync`. |
| `initial sync: …` | Starting state established. Keep the printed backup if the old local copy contained edits. |
| `already up to date …` | Nothing needed sending or downloading. |
| `push: accepted …` | Your supported edits were accepted and the local catalogue refreshed. |
| `pull: local copy updated …` | A newer server catalogue was downloaded; the previous copy was backed up. |
| `conflicting user edits preserved in backup …` and `conflict: …` | Some edits need reconciliation. Read the recovery steps below. |
| `sync deferred: …` | The server could not be reached. Keep working locally and run sync again when it is available. |

Read the output even when the command exits successfully: deferred sync and reported conflicts are normal results, not proof that every edit reached the server. Keep the backup paths printed by sync until you have checked the result.

Use the real catalogue file path for sync. A symlink is refused; on Unix and
Windows, a catalogue with multiple hard links is also refused. Replacement would leave
the other pathname stale (D74/D81).

Uploads contain user edits, but refreshing a catalogue currently downloads the whole SQLite catalogue. A small rating change can therefore be followed by a much larger download, particularly with an embedded preview store.

## Working offline

After cloning, or the first successful sync of a manual copy, your local catalogue remains usable without the server. You can browse stored thumbnails and metadata and edit annotations and Collections. Run the same sync command when connected again; pending edits survive closing and reopening the app.

Two clients can work independently offline. Edits to different assets or Collections normally both succeed. Changes to the same asset's rating/label/keyword set can conflict, even when the two people changed different fields. Name, rules and membership changes within one Collection are also treated as one unit for conflict detection.

## Recovering conflicting edits or pre-first-sync work

When a conflict occurs, the active local catalogue is refreshed from the server. Your draft survives in the backup identified by the output, and a recovery journal stores both sides and any choices beside the catalogue. Other non-conflicting edits from the same sync can still be accepted. The desktop shows each asset by filename and each Collection by name, alongside its stable key. It shows changed fields with both values and the values that match on both sides as read-only shared context. Conflicts are paged, with previous/next controls; the desktop shows the current position. Keyword sets and manual Collection membership also allow a combined choice.

1. Keep the backup file and note the reported conflict keys. They are stable identities, which may differ from the numeric IDs shown by `list`.
2. In the desktop conflict recovery view, inspect each field's local and server value and choose **Local**, **Server** or **Combined** where offered. Choices persist if the app closes.
3. Choose **Apply choices and sync**. iCatalog applies all selected values together while retaining the server's values in fields you did not select, then retries sync. If the retry cannot complete, keep the backup and journal and retry after fixing the reported issue.

For CLI recovery, start with a page of conflicts:

```sh
icatalog sync-resolve Library.icatalog --action list --page 0 --page-size 50
```

The output includes a session ID, total and page counts, readable asset/Collection names, differing local/server values, saved choices, and shared fields. Pages start at zero; `--page-size` defaults to 50 and accepts 1–200. Use that session ID with each later action:

```sh
icatalog sync-resolve Library.icatalog --action show --session-id SESSION --key 'asset:UID'
icatalog sync-resolve Library.icatalog --action choose --session-id SESSION --key 'asset:UID' --field rating --choice local
icatalog sync-resolve Library.icatalog --action apply --session-id SESSION --url https://nas:8443
```

Use `--action choose` for every differing field, with `--choice local`, `server` or `combined`; the combined choice is supported for keyword and manual member sets. Later `list` pages can include the same `--session-id`. Stale session IDs are rejected. `apply` also accepts `--token-file` and `--ca-file`, applies all saved choices transactionally and immediately retries sync. The journal is stored in `<catalogue>.sync-state.sqlite` and remains available after a failed attempt. Conflicts for entities deleted from the server currently stop with local work preserved and cannot be resolved through this workflow. Do not replace the current copy with a stale backup; that can reproduce the same conflict.

If you need to map a reported conflict key to a filename or Collection name, these optional read-only queries require the separate `sqlite3` command. Replace `ASSET_UID` with the part after `asset:` or `COLLECTION_UID` with the part after `collection:` in the reported key; replace the backup path too:

```sh
sqlite3 -readonly "/path/to/Library.rejected-TIMESTAMP.icatalog" \
  "SELECT id, name, path FROM asset WHERE uid='ASSET_UID';"
sqlite3 -readonly "/path/to/Library.rejected-TIMESTAMP.icatalog" \
  "SELECT id, name FROM collection WHERE uid='COLLECTION_UID';"
```

If a UID contains a single quote, double that quote inside the SQL string. The `sqlite3` tool is only needed for this lookup; ordinary iCatalog use does not require it.

## Updating the server's media

On the server, rescan its catalogue after adding or changing media:

```sh
icatalog update "$HOME/iCatalog/Library.icatalog"
```

Import an additional media directory with `icatalog import … --path …`. If you use larger previews, regenerate them after updating:

```sh
icatalog previews "$HOME/iCatalog/Library.icatalog" generate
```

Clients receive the catalogue changes on their next sync. Generate reports unavailable, changed or unsupported source files; make sure the originals are accessible and run Update before retrying changed files.

## Larger previews

Use `icatalog previews <catalogue> status` on the server to see its storage mode and count. `external` stores JPEG files beside the catalogue; `embed` copies referenced preview bytes back inside SQLite for a portable file. Conversion verifies the bytes before committing and retains external files afterward.

The desktop full-size preview viewer is not built yet. For CLI/API use, a client can fetch one known preview hash:

```sh
icatalog sync "$HOME/Pictures/iCatalog/Library.icatalog" --url http://nas:8080 --preview "<64-character SHA-256>"
```

Replace the placeholder with the desired preview's hash; in an external server store, it is the filename without `.jpg`. Run ordinary catalogue sync first so the local copy knows that preview. This command fetches that preview only—it does not send pending edits or refresh the catalogue. Valid cached bytes are reused. Generating and converting previews happens on the server, where the originals are available.

## Troubleshooting

| Problem | What to check |
|---|---|
| HTTP 401 | Select the correct token file for this server; after token rotation, update the client copy. Local edits remain pending. |
| TLS/certificate error | Use the certificate's hostname, check expiry and the machine clock, and select the correct private CA file if needed. Never bypass verification. |
| Clone cannot install the downloaded file | Ensure the parent directory is writable and the local filesystem supports hard links; check whether a completed catalogue already exists before retrying. |
| Catalogue download returns HTTP 500 | Check the server terminal. Its catalogue or containing folder may have been moved, trashed or become unwritable while the service kept an old SQLite handle open. Stop the service, restore the catalogue to an existing writable folder, and restart `serve` with its correct path; then retry Clone. Do not initialise a replacement over an existing catalogue. |
| Clone destination already exists | Choose a new filename, or use `sync` for an existing replica. Do not delete a catalogue or its sidecars to force clone over local work. |
| Server unreachable / sync deferred | Service still running, correct hostname/IP and port, firewall access, and a LAN `--bind` address rather than the default localhost-only listener. |
| Catalogue ID mismatch | The client must start from the server's catalogue. Preserve the existing file and use `clone` at a separate local path. Do not change IDs to force two unrelated catalogues to match. |
| Protocol version error | Upgrade both server and client to builds supporting protocol v2. |
| Database locked or old-connection error | Close the desktop app and other tools using that local catalogue, then retry and reopen the app. |
| Import/Update unavailable on a client | Run those commands against the main server catalogue. |
| Backup, disk-space or download verification error | Keep the current local file and existing backups, resolve the reported problem, then retry sync. Avoid copying an unverified download over your catalogue. |

For implementation details, transfer limits and exact recovery guarantees, see the [sync protocol reference](sync.md).
