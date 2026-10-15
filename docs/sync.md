# iCatalog sync protocol

Purpose: full sync model and conflict policy (summary: `AGENTS.md` section 5). Status vocabulary: **Decided** (D1/D2/D18 or a golden rule), **Implemented** (built and tested; see the section notes), **Proposed** (designed here, not built), **Open** (unresolved). The whole-snapshot `serve`/`sync` slice is **Implemented** in `crates/icatalog-sync` (D18); TLS, auth, delta transfer and merge are **Open**.

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

## 3. HTTP(S) API

**Implemented (D18), plain HTTP only.** Paths, status codes and payload encoding are fixed; TLS and auth are **Open**.

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

**Decided (D18).** A push is refused if the server generation moved since the client's base generation (**refuse-and-resync**, never silent last-writer-wins). **Implemented.**

Sequence (**Implemented**):

1. The client decides from the generation: server ahead -> back up and **pull**; level -> **push**; client ahead of the server -> **refuse** (never overwrite the server).
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

Tests: synthetic and round-trip tests **exist** (`crates/icatalog-sync/tests/sync.rs`): a real push/pull round trip against the service, a refused push (backup + resync), an invalid snapshot that leaves the local copy untouched, offline deferral, and hostile-parameter coverage.

## 7. wasm browser client

**Decided (D3):** the browser keeps a local copy and syncs over the same HTTP(S) API. The client sits behind a small `Transport` trait (`crates/icatalog-sync/src/transport.rs`) so a wasm `fetch` implementation can replace the native `ureq` one later. **Open:** the storage backend (e.g. OPFS), running SQLite in wasm, and how the temp-file-and-swap sequence maps to browser storage. Mixed-content and CORS rules for the API are also **Open**. The server half (`tiny_http`) is native-only.

## 8. Open questions

- Is the sync service **mandatory**, or is there also a simple **share-file mode**? (open in `docs/decisions.md`)
- Auth and TLS (section 3).
- Incremental sync versus whole-snapshot.
- Merge strategy and stable item ids.
- Browser storage.
