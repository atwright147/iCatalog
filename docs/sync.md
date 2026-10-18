# iCatalog sync protocol

Purpose: full sync model and conflict policy (summary: `AGENTS.md` section 5). Status vocabulary: **Decided** (D1/D2/D18 or a golden rule), **Implemented** (built and tested; see the section notes), **Proposed** (designed here, not built), **Open** (unresolved). The whole-snapshot `serve`/`sync` slice is **Implemented** in `crates/icatalog-sync` (D18), including the local change marker that makes an idle re-sync a no-op (D19); TLS, auth, delta transfer and merge are **Open**.

**Read this first:** the *Target model* below (D20) is the decided direction — annotation-delta push, previews pulled not pushed, a per-catalogue preview store. It is **not implemented**. Sections 1–8 describe what is built today (the D18/D19 bidirectional whole-snapshot push) and are marked where D20 supersedes them.

## Target model (decided in D20; not implemented)

**Nothing in this section is built.** It records the owner's decided direction (D20) and the points still open.

- **One writer, many readers.** The server (NAS `icatalog serve`) is authoritative for the catalogue and the media; clients pull.
- **Push is an annotation delta, not a snapshot.** A client sends only user annotations — rating, label, keyword membership — keyed by stable asset `uid`. Never media, thumbnails or previews. This replaces `POST /catalogue`.
- **Pull is the catalogue** (assets, `metadata_raw`/EXIF, annotations, grid thumbnails). The download / verify / back-up / atomic-swap sequence (section 5) is unchanged.
- **Previews are content-addressed and pulled only.** `GET /preview/<sha>` returns one preview; the server derives them from the media it holds, a client fetches only what it lacks, and a client never pushes a preview.
- **The preview store is a per-catalogue choice, one format.** Preview bytes are either **embedded** in the catalogue file (a standalone, portable single file — the `~/Pictures` case) or **external** in `previews/<sha>.jpg` beside it (the NAS case, so a client pull stays small and previews arrive lazily). Grid thumbnails stay in the catalogue in both modes.
- **A catalogue may be standalone or a replica.** A standalone catalogue simply never runs `sync`; no new flag beyond the preview-store setting.
- **Single file stays single.** The catalogue uses a rollback journal (no WAL), so there are no `-wal`/`-shm` sidecars. Enabling WAL later requires a checkpoint on close or a `VACUUM INTO` export.

Proposed, not accepted (**Open**): per-item compare-and-swap for annotation pushes (a `base_rev` per asset) replacing the generation guard; push-then-pull when a client has unpushed edits; a `previews --external|--embed` conversion command; a shared-token auth on the write endpoint. Also **Open:** preview sizes and quality, whether the write endpoint survives, and how a push names a keyword the server does not yet have.

## 1. Model

**Decided (D1, D2).**

- Each computer keeps a **local SQLite copy** and opens only that.
- The **NAS holds the canonical copy**, owned by `icatalog serve`, which serialises writes. SQLite directly on SMB/NFS was rejected: network-share locking is unreliable and corrupts databases.
- Clients (CLI, desktop GUI, wasm browser build) sync over an **HTTP(S) API**.
- Sync runs at startup and on explicit `icatalog sync`. Sync is a command, so CLI and GUI behave identically.
- **Offline:** fully usable. If the NAS is unreachable, sync is deferred and reported, never fatal. **Implemented:** the client reports `sync deferred: ...` and leaves the local copy unchanged.
- "Everyone sees the same thing" means sync points, not live co-editing.

## 2. Identity and generation

**Decided:** the catalogue has a per-catalogue **id** and a **generation counter** bumped on every accepted push. **Implemented.**

- The client records the **base generation**: the server generation it last pulled or successfully pushed (its local `catalogue.generation`).
- Local edits do not change the generation counter; only an accepted push does.
- The catalogue id guards against syncing unrelated catalogues; a mismatch is refused.

**Local change marker (implemented, D19).** Because local edits do not move the generation counter, the client keeps a small **local-only** marker beside its catalogue: `name.sqlite.sync-state`, holding `fingerprint=<hash>` — a fast non-cryptographic fingerprint of the local file after the last successful push or pull. On a level sync the client compares the marker to the current file: a match means nothing changed, so it is a **no-op** (no upload, no generation bump); a mismatch (or a missing marker) means the local copy changed and it is pushed. The marker is never sent to the server and never affects the catalogue id or generation. It is best effort: a missing or unreadable marker only causes an extra push, never a lost one.

## 3. HTTP(S) API

**Implemented (D18), plain HTTP only.** Paths, status codes and payload encoding are fixed; TLS and auth are **Open**. **Superseded in part by D20:** the push endpoint becomes an annotation delta (`POST /annotations`) and `GET /preview/<sha>` is added; not implemented.

| Operation | Request | Response |
|---|---|---|
| Info | `GET /info` | `200`, body `id=<id>\ngeneration=<n>\n` |
| Pull | `GET /catalogue` | `200`, raw SQLite bytes, headers `X-Catalogue-Id`, `X-Generation` |
| Push | `POST /catalogue?expected=<base>`, body = raw SQLite bytes | `200` + `X-Generation: <new>` on accept, or `409` + `X-Generation: <current>` when the generation moved |

Whole-snapshot transfer is the implementation; incremental sync and per-item merge are later work (**Open**).

Guards (**Implemented**): the server caps a snapshot body (4 GiB) and verifies an incoming snapshot (it must open as an iCatalog catalogue with the matching id, and pass `PRAGMA integrity_check`) before it replaces anything; any other `POST` state (bad/missing `expected`, oversized or invalid body) is refused with `400`/`413`.

**Open:**

- **Auth:** none, shared token, per-user credentials? Undecided.
- **TLS:** self-signed, user-supplied certificate, or plain HTTP on a trusted LAN? Undecided. `ureq` is built without TLS in this slice, so `https://` is rejected with an "unsupported" error.
- Versioning of the API and of the catalogue schema.
- Incremental (delta) transfer; the whole file is sent today.

## 4. Conflict policy

**Decided (D18).** A push is refused if the server generation moved since the client's base generation (**refuse-and-resync**, never silent last-writer-wins). **Implemented.** **Superseded in part by D20:** the push direction and this coarse generation guard give way to a per-item compare-and-swap on annotation deltas (not implemented).

Sequence (**Implemented**):

1. The client decides from the generation: server ahead -> back up and **pull**; level -> **push** unless the local copy is unchanged since the last sync (then a **no-op**, D19); client ahead of the server -> **refuse** (never overwrite the server).
2. On a push refusal (a race: another client pushed between info and push) the client writes its local state to a timestamped backup `name.rejected-<UTC>.sqlite` beside the local copy, then resyncs from the server.
3. The refusal, the backup path and the new generation are reported to the user.

Merging local changes (per-item, by stable item id) is a later improvement and must keep the same safety guarantees. **Open:** the merge strategy and stable item ids.

## 5. Atomic download and swap

**Decided** principles; **Implemented** sequence:

1. Download to a temp file in the same directory as the local copy (so the rename stays on one filesystem).
2. Verify: it opens as an iCatalog catalogue, the id matches, and `PRAGMA integrity_check` returns `ok`.
3. Back up the existing local copy first (timestamped) before any overwrite.
4. Close the SQLite connection and atomically rename the temp file over the local copy, then reopen it.
5. Any failure before step 4 leaves the existing local copy untouched; the temp file is removed.

## 6. What "no silent data loss" means

**Decided** operationally:

- A failed or interrupted sync never destroys the only copy.
- Nothing is replaced before the replacement is verified.
- A rejected local state is always kept as a backup before it is overwritten.
- Every refusal or deferral is reported to the user.
- Server-side writes are transactional (SQLite) and serialised by the single service loop.

Tests: synthetic and round-trip tests **exist** (`crates/icatalog-sync/tests/sync.rs`): a real push/pull round trip against the service, a refused push (backup + resync), an invalid snapshot that leaves the local copy untouched, offline deferral, hostile-parameter coverage, an idle re-sync that is a no-op, and a local change that is pushed.

## 7. wasm browser client

**Decided (D3):** the browser keeps a local copy and syncs over the same HTTP(S) API. The client sits behind a small `Transport` trait (`crates/icatalog-sync/src/transport.rs`) so a wasm `fetch` implementation can replace the native `ureq` one later. **Open:** the storage backend (e.g. OPFS), running SQLite in wasm, and how the temp-file-and-swap sequence maps to browser storage. Mixed-content and CORS rules for the API are also **Open**. The server half (`tiny_http`) is native-only.

## 8. Open questions

- Is the sync service **mandatory**, or is there also a simple **share-file mode**? (open in `docs/decisions.md`)
- Auth and TLS (section 3).
- Incremental sync versus whole-snapshot.
- Merge strategy and stable item ids.
- **D20 (target model):** preview store mode (embedded vs external), preview sizes and quality, per-item conflict granularity, the `previews --external|--embed` command, and whether the write endpoint survives. See the Target model section.
- Browser storage.
