# iCatalog sync protocol

Purpose: full sync model and conflict policy (summary: `AGENTS.md` section 5). Status vocabulary: **Decided** (D1/D2 or a golden rule), **Proposed** (designed here, not accepted or built), **Open** (unresolved). Nothing here is implemented; `icatalog-sync` does not exist.

## 1. Model

**Decided (D1, D2).**

- Each computer keeps a **local SQLite copy** and opens only that.
- The **NAS holds the canonical copy**, owned by `icatalog serve`, which serialises writes. SQLite directly on SMB/NFS was rejected: network-share locking is unreliable and corrupts databases.
- Clients (CLI, desktop GUI, wasm browser build) sync over an **HTTP(S) API**.
- Sync runs at startup and on explicit `icatalog sync`. Sync is an engine command, so CLI and GUI behave identically.
- **Offline:** fully usable. If the NAS is unreachable, sync is deferred and reported, never fatal.
- "Everyone sees the same thing" means sync points, not live co-editing.

## 2. Identity and generation

**Decided:** the catalogue has a per-catalogue **id** and a **generation counter** bumped on every accepted push.

**Proposed semantics:**

- The client records the **base generation**: the server generation it last pulled or successfully pushed.
- Local edits do not change the generation counter; only an accepted push does.
- The catalogue id guards against syncing unrelated catalogues; a mismatch is refused.

## 3. HTTP(S) API (Proposed, concept only)

**Open:** endpoint paths, status codes, payload encoding, auth and TLS. The set below is a conceptual sketch.

| Operation | Request | Response |
|---|---|---|
| Info | none | catalogue id, current generation, size, integrity hint |
| Pull | optionally client's base generation | the catalogue snapshot at a generation (whole file first; changed pages or deltas are **Open**) |
| Push | catalogue snapshot + **expected generation** (the client's base) | accepted with new generation, or refused because the generation moved |
| Backup upload | rejected local state | server-side copy kept (optional, **Open** whether wanted) |

Whole-snapshot transfer is the simplest start (**Proposed**); incremental sync and per-item merge are later work.

**Open:**

- **Auth:** none, shared token, per-user credentials? Undecided.
- **TLS:** self-signed, user-supplied certificate, or plain HTTP on a trusted LAN? Undecided.
- Versioning of the API and of the schema (see `docs/schema.md`).
- Large-catalogue transfer: resumable downloads, compression.

## 4. Conflict policy

**Decided:** a push is **refused** if the server generation moved since the client's base generation. **Refuse-and-resync**, never silent last-writer-wins.

Sequence on refusal (**Decided** in outline, details **Proposed**):

1. The client saves its rejected local state as a **timestamped backup**, e.g. `name.rejected-<UTC timestamp>.sqlite` (naming from `AGENTS.md`; location, next to the local copy, is **Proposed**).
2. The client resyncs from the server using the atomic download in section 5.
3. The user is told that a conflict happened and given the backup path.

Merging local changes (per-item, by stable item id) is later work and must keep the same safety guarantees. Until then conflicts are surfaced, not merged. **Open:** the stable item id scheme.

## 5. Atomic download and swap

**Decided** principles: downloads land in a temp file, are integrity-checked, then swapped in atomically. **Proposed** sequence:

1. Download to a temp file in the same directory as the local copy (so the rename stays on one filesystem).
2. Verify: SQLite integrity check, catalogue id matches, generation matches the server's response.
3. If a local copy with unpushed changes exists, back it up first (timestamped).
4. Atomically rename the temp file over the local copy.
5. Any failure before step 4 leaves the existing local copy untouched; the temp file is removed.

## 6. What "no silent data loss" means

**Decided** operationally:

- A failed or interrupted sync never destroys the only copy.
- Nothing is replaced before the replacement is verified.
- A rejected local state is always kept as a backup before it is overwritten.
- Every refusal or deferral is reported to the user.
- Server-side writes are transactional (SQLite).

Tests required: synthetic and round-trip tests, including interrupted transfers and generation races.

## 7. wasm browser client

**Decided (D3):** the browser keeps a local copy and syncs over the same HTTP(S) API. **Open:** the storage backend (e.g. OPFS), running SQLite in wasm, and how the temp-file-and-swap sequence maps to browser storage. Mixed-content and CORS rules for the API are also **Open**.

## 8. Open questions

- Is the sync service **mandatory**, or is there also a simple **share-file mode**? (open in `docs/decisions.md`)
- API shape, auth, TLS (section 3).
- Incremental sync versus whole-snapshot.
- Merge strategy and stable item ids.
- Browser storage.
