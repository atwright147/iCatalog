# iCatalog sync protocol

For setup and everyday use, start with [Using iCatalog sync](sync-user-guide.md). This document describes the protocol and recovery guarantees.

D20 is implemented in HTTP protocol v2, including Collection edits requested in D44. Schema v7 introduced sync state; schema v8 adds Maximum Rating rules (D57). The server owns media-derived catalogue data; replicas upload ratings, labels, keywords and Collections. Catalogues and previews travel server to client. D18/D19 snapshot uploads and fingerprint sidecars are retired. TLS/auth, incremental catalogue downloads, automatic desktop startup sync and a desktop full-size preview viewer remain unbuilt.

## Model and identity

Each computer opens a local SQLite copy. `icatalog serve` owns the canonical catalogue on the NAS; `icatalog sync` pushes supported local edits before pulling the latest catalogue. SQLite is never shared directly over SMB/NFS. An unreachable server defers catalogue sync without replacing the local copy.

Catalogue IDs must match. Assets use their existing stable `uid`. Collections now also have immutable UIDs: migration v7 derives existing IDs deterministically from catalogue ID and numeric collection ID, so independently migrated copies agree; new Collections receive random 128-bit IDs. Numeric Collection IDs remain local database keys and CLI handles; wire membership uses asset UIDs. Concurrently created Collections cannot collide merely because their numeric IDs match.

The generation advances transactionally on writes to catalogue data tables, including direct catalogue CRUD and server-side ingest/updates. It can advance several times in one command; it is an increasing change counter, not a command count. Internal replica-baseline bookkeeping does not advance it. Revision and generation overflow abort the triggering write. Media/metadata and preview settings on an established replica are server-managed; Import/Update are disabled in the desktop and engine. Ratings, labels, keywords and Collection editing remain available offline.

## Establishing a replica and tracking edits

`icatalog clone <new-local-file> --url http://nas:8080` (D45) creates a ready-to-edit replica directly from a live server. It discovers the catalogue identity, shares sync's verified download/baseline preparation and installs the completed file without overwriting any destination entry. The CLI prepares an in-memory destination session, so errors cannot create/migrate a file through normal session opening. Parent directories must exist; existing destinations, SQLite sidecars, and the bare `:memory:` path are refused before transfer (D69). Downloads are staged beside the target, flushed, and published with an atomic no-overwrite hard link (the local filesystem must support hard links). No upload, schema change or new endpoint is involved; the CLI does not remember URLs. Failed downloads/verification remove their staging file and leave the destination absent; a competing destination is preserved. Publication/flush/reopen errors can leave a verified completed file, which is retained rather than deleted.

A clone records the baseline immediately, so an idle sync is a no-op and the first subsequent user edits can be uploaded. The following first-sync backup behavior remains for manually copied or older local files.

A copied catalogue has no trustworthy record of its last synced annotations. On its **first v2 sync**, iCatalog verifies/downloads the server catalogue, preserves the previous local file in a timestamped backup, then establishes a replica baseline inside the downloaded SQLite file. It uploads nothing on this first sync. The CLI explicitly reports this and the backup path. This also applies to copies previously synced with D18/D19: their old `.sync-state` fingerprint cannot establish a per-item baseline and is ignored.

Afterward `replica_base` stores the last server generation plus a bounded binary baseline of user-editable state. Comparing current values with that baseline identifies pending edits, even after restarting the app. Local revision changes never masquerade as server revisions. An idle sync sends no upload, downloads no catalogue and does not advance the server generation. No additional baseline sidecar is created.

Do original imports and rescans on the server or a standalone catalogue. To start a replica, use `clone` and then edit, or manually copy the server catalogue and run `sync` before editing. Existing local-only work remains recoverable from the first-sync backup; it is not automatically merged without a known base.

## Conflict policy and retries

Annotation changes carry the stable asset UID, the baseline revision and the complete desired rating/label/keyword-name set. A revision covers that entire set, so two different fields on the same asset can conflict. A new keyword is named directly and created by the server when needed. Existing captions, headlines, creators and copyright fields remain intact when a replica edits a rating or label; those text fields and all media/thumbnail/metadata data are excluded from uploads.

Collection changes carry UID, baseline revision, name, optional typed Smart Collection rules and sorted manual member asset UIDs. New Collections have no baseline revision. Each Collection is one conflict unit: concurrent name/rule/membership edits to the same Collection can conflict. Edits to separate Collections or assets proceed independently. Smart membership remains computed; it is never uploaded as a member list. Collection deletion and changing a Collection between manual and Smart are unsupported.

The server validates the whole batch, then checks revisions under a SQLite `BEGIN IMMEDIATE` transaction. Matching entities are applied; stale or missing entities and missing Collection members are reported as conflicts. If a storage operation fails, all writes from that batch roll back. A retry whose desired value is already present is accepted as a no-op, including retries after the server committed but its reply was lost. No silent last-writer-wins occurs.

After uploading, the client pulls the current catalogue. Conflicting drafts are preserved in the pre-replacement backup and their stable keys are printed; the active copy shows server values. Other accepted entities remain applied. A failed upload/download leaves the local baseline and pending work intact; retrying cannot duplicate new Collections or reapply an already accepted value. Restoring a backup also restores its baseline and drafts; reconciliation is manual when edits still conflict.

## HTTP endpoints

Plain HTTP is implemented using `tiny_http` and `ureq`. Upgrade both peers for protocol v2; clients reject an old server before uploading, and the new server rejects old snapshot uploads. Authentication and TLS are separate future work.

| Request | Result |
|---|---|
| `GET /info` | UTF-8 lines `protocol=2`, `id=<catalogue-id>`, `generation=<nonnegative integer>` |
| `GET /catalogue` | Consistent SQLite snapshot; `X-Catalogue-Id` and `X-Generation` describe that exact snapshot |
| `POST /annotations` | Bounded binary v2 batch of annotation and Collection changes; 200 reply includes generation and conflict keys; malformed/wrong-ID batches return 400; oversized batches return 413 |
| `POST /catalogue` | 405; snapshot uploads are retired |
| `GET /preview/<sha>` | Referenced JPEG bytes; hash must be exactly 64 lowercase hexadecimal characters; invalid hash 400, unavailable/unreferenced hash 404 |

`GET /info` is capped at 8 KiB, snapshots at 4 GiB, edit messages/baselines at 64 MiB and previews at 16 MiB. User-state reads are bounded to 100,000 assets/Collections and 100,000 keywords or members per entity, subject also to the total byte bound. Oversized catalogues return an actionable error rather than allocating from untrusted counts. Redirects are disabled; connection and total request timeouts are bounded.

### Binary edit encoding

`crates/icatalog-sync/src/delta.rs` is the canonical codec, with round-trip, truncation and mutation tests. It adds no dependency. Integers are little-endian: signed i64 revisions/ratings/labels, unsigned u32 lengths/counts. Text is length-prefixed UTF-8; lists begin with a count. Optional text has a 0/1 byte followed by text when present. `-1` represents an absent optional integer/base revision; other negative values are invalid.

A batch starts with six bytes `ICAD2\0`, catalogue-ID text and a record count. Each record has kind byte (1 annotation, 2 legacy Collection, 3 Collection with maximum-rating rules), baseline revision, an informational current revision and UID. Annotation data is rating, label and a sorted unique keyword list. Collection data is name, 0/1 Smart-rule presence, rules when present, then a sorted unique member-UID list. Rule text order is format, captured-from, captured-to, camera, lens, keyword, text; it is followed by optional minimum rating and label for kind 2. Kind 3 inserts optional maximum rating between minimum rating and label, and requires Smart rules with a maximum present. Optional numeric values use -1 for absence. Encoders emit kind 3 only when maximum rating is present; manual Collections and Smart rules without a maximum retain the exact kind 2 representation. Smart rules reuse normal catalogue validation. New decoders read existing kind 2 replica baselines as having no maximum. Older decoders reject unknown kind 3 before applying any edits; failed uploads retain local drafts and baseline. Schema v8 preserves baseline bytes during migration. Upgrade both server and clients before sharing v8 catalogues: older apps reject the newer schema, and older servers reject maximum-rule uploads instead of silently discarding the bound. A reply begins `ICAR2\0`, generation and a list of conflict keys (`asset:<uid>` or `collection:<uid>`). Trailing bytes, duplicate entity keys, invalid flags/ranges, unbounded counts and unsorted/duplicate lists are rejected.

The same record encoding stores the embedded baseline. Baselines contain only user-editable values and revisions, not media, thumbnails or preview bytes. The server does not trust the uploaded informational revision; compare-and-swap uses the baseline revision against its own stored revision.

## Download, backup and replacement safety

The server creates snapshots through SQLite `VACUUM INTO`, so concurrent NAS-side commands cannot produce a torn file and freed embedded-preview pages do not inflate external-store downloads. Snapshot headers are read from that snapshot, not from an earlier `/info` response.

The client requires DELETE journal mode and holds a local SQLite write reservation while inspecting/uploading/downloading. Other SQLite writers cannot change the state being synchronized during these requests. The download goes into an exclusively reserved temporary file beside the local catalogue. It must pass:

- SQLite integrity and foreign-key checks;
- expected catalogue ID and matching header/database generation;
- generation at least as new as the acknowledged upload/discovery;
- an authoritative catalogue with no replica baseline;
- valid, bounded user state for the new baseline.

The client establishes the baseline in the verified temporary database and flushes it. Before replacement it creates and flushes a uniquely named `<stem>.rejected-<UTC timestamp>[-n].sqlite` backup without overwriting an existing file. Failure to back up aborts replacement. It closes its old connection, atomically renames the verified file into place and reopens it. Temporary files are removed on failure. No original media or preview file is deleted by sync. A server generation moving backward relative to a replica baseline is refused.

## Preview storage and commands

Grid thumbnails stay embedded. A `preview` row links an asset to a SHA-256 JPEG hash, oriented dimensions and source content hash. Previews use a 1920-pixel maximum long edge, JPEG quality 80, no upscaling and the existing EXIF orientation/media decoder; RAW uses its embedded JPEG preview without demosaicing. Generation is explicit and runs on the machine holding the original media:

```sh
task cli:previews -- photos.sqlite status
task cli:previews -- photos.sqlite generate
task cli:previews -- nas.sqlite external
task cli:previews -- nas.sqlite generate
task cli:previews -- photos.sqlite embed
```

The per-catalogue default is **embedded**, giving a standalone portable SQLite file usable without the originals. **External** stores verified bytes in `previews/<sha>.jpg` beside the catalogue; choose this for a NAS so catalogue downloads contain references and grid thumbnails, while previews arrive lazily. External directories must be real directories, not symlinks. Writes use exclusive temporary files and rename; filenames derive only from validated hashes.

Conversion verifies every referenced blob before committing the new mode. Missing/corrupt bytes or write failures leave the previous database representation intact. External files are retained after embedding and harmless unreferenced files may remain after failed generation/conversion; automatic garbage collection is not implemented. Updating an asset's content hash invalidates its old preview reference. Missing/changed/unsupported source files are counted and reported by Generate; changed sources require Update first.

A replica fetches a referenced preview explicitly with:

```sh
task cli:run -- sync local.sqlite --url http://nas:8080 --preview <sha>
```

This performs GET only, verifies SHA-256 before caching and makes no request when valid bytes are already cached. It uses the catalogue's storage mode and never opens catalogued original-media paths. There is no preview upload endpoint. Desktop Connect/Clone and explicit Sync controls exist (D46); automatic startup sync and a full-size preview viewer are not wired yet; the desktop continues to display grid thumbnails.

## Remaining work

TLS/authentication; automatic desktop startup sync; incremental catalogue downloads; per-field conflict merging; Collection deletion/conversion; preview garbage collection and GUI viewing; the wasm transport/storage implementation; container registry publication and service installation. D58 supplies static x86-64/ARM64 Linux musl CLI build/archive tasks and CI jobs; D60 adds a scratch image, Compose setup and disposable runtime checks (see the [NAS Docker guide](nas-docker.md)). New hosted runs/publication remain unverified. The HTTP service and v2 protocol can be tested locally independently of that packaging.

The CLI `serve` session opens an existing on-disk catalogue without SQLite's create
flag (D60). Missing paths fail without creating a replacement catalogue, including
when a container mount is wrong. Initialization remains an explicit `init` command;
schema migration and protocol behavior are unchanged.

## Desktop client

D46 exposes the existing `clone` and `sync` commands through native File-menu dialogs. Operations run on a worker; the desktop closes its catalogue reader during sync and reopens/refreshes after completion. Full command output, offline deferrals and conflict backup paths remain visible. Untracked first sync requires acknowledging the backup-first policy. The desktop remembers an address locally in `<catalogue>.sync-url`; this optional preference never travels inside catalogue snapshots and does not alter CLI `--url` requirements. Sync remains explicit. See the [user guide](sync-user-guide.md) for the click-by-click workflow.

Snapshot preparation failures return HTTP 500 with a text explanation instead of an empty response. The server checks that its configured catalogue path is still a file before taking a snapshot; a running SQLite handle can otherwise survive moving the file or its containing folder to the Trash. Restore the catalogue to a writable folder and restart the service at the correct path before retrying. Clients report HTTP status and bounded server diagnostics (including guidance for older empty error responses); failed clone/download still preserves the current catalogue and leaves new destinations absent. No protocol version or endpoint change.
