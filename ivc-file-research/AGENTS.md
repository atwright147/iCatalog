# AGENTS.md: ivc-file-research guide (legacy `.ivc` / `.mpcatalog` format)

This folder is the **clean-room reverse engineering** of the legacy iView MediaPro / Expression Media / Phase One Media Pro catalogue format (`.ivc` / `.mpcatalog`). We study the product lineage for **behaviour and the on-disk format only, never code or assets**. The app that consumes this work, iCatalog (a native, Rust-only media catalogue; SQLite is its own format, the legacy format is import-only), lives at the repo root (`../AGENTS.md`). **Rust only: no JavaScript or TypeScript, no Tauri/Electron/webview, ever.**

**Conventions:** all paths below are repo-root-relative (e.g. `ivc-file-research/samples/`). The guide for app development is `../AGENTS.md`. Read this file first, then the spec.

**Shared facts:** the repo is not under git yet and has no repo-level `.gitignore` (only `ivc-file-research/samples/.gitignore`). There is ONE Cargo workspace rooted at the repo root (`Cargo.toml` at the root, `resolver = "2"`, `members = ["ivc-file-research/crates/ivc"]` today; `crates/*` is added when the first app crate exists), so `cargo test --workspace` and `cargo clippy --workspace` are the gate for both parts.

## 1. Format at a glance

Big-endian. Header = u32be item count + magic `025i` / `030i`. Then N variable-length item records: a 1024-byte head (Pascal path @+0x000, filename @+0x100, 4cc format @+0x14c, size/width/height @+0x160..0x168, colour space @+0x398, PICT length @+0x3f4), then a QuickDraw PICT v2 thumbnail @+0x400 (often embedding JFIF), then tail blocks + IPTC. Then chunks (`4cc type + u32be length excluding the 8-byte header + payload`): `FSM!` folder tree, `CELL`, `BLDO`, `USF3`, `CMRS`, `CSTT`, `TOTL`, `FONT`, `PREF`, `SVar`, `MSOP`/`MSVI`... Trailer = magic + absolute first-chunk offset. The spec marks sections CONFIRMED / PARTIAL / UNKNOWN with byte ranges.

## 2. What exists

| Path | Status | Why |
|---|---|---|
| `ivc-file-research/docs/ivc-format.md` | EXISTS | byte-level spec (with a References section: sample provenance, archived official manual and binaries, PRONOM fmt/647); its CONFIRMED/PARTIAL/UNKNOWN discipline and §6 UNKNOWN summary are the live work queue |
| `ivc-file-research/samples/` | EXISTS | 16 real iView/Expression/Phase One catalogue files, our test oracles. **Read-only: never modify.** Provenance: samples courtesy of Tyler Thorsted's PRONOM_Research project (https://github.com/thorsted/PRONOM_Research/tree/main/Submissions/iView/Samples), licensed **CC0 1.0** (public domain). The samples are committed (CC0 1.0). `ivc-file-research/samples/fetch.sh` + `ivc-file-research/samples/SHA256SUMS` are retained as an optional re-fetch/verifier (pinned commit); run `fetch.sh` if the corpus is missing. It is not required before `cargo test -p ivc` |
| `ivc-file-research/crates/ivc/` | EXISTS | L0 parser: `src/lib.rs`, `tests/samples.rs` (oracle suite), `examples/dump.rs` |

`ivc` is the format-knowledge crate: standalone, **no workspace dependencies**. The app consumes it via a path dependency. It keeps the name `ivc` because it is format-specific, not app-branded.

## 3. Golden rules

- **Never panic on hostile bytes.** `ivc` has no `unwrap()`, `expect()`, `panic!`, `unreachable!`, `todo!`, `unimplemented!` and no `unsafe` (the workspace sets `unsafe_code = "forbid"`). Return the crate's error type and propagate with `?`. Check the crate for existing `#![deny(clippy::unwrap_used, ...)]` attributes before assuming; new code must comply either way.
- **Input-derived numbers are hostile.** Use `get()` rather than `[i]`/`[a..b]` for offsets from the file; use `checked_*`/`saturating_*` for lengths, offsets and counts (the spec is full of u32 lengths and absolute offsets); cap allocations sized by input; bound recursion (the `FSM!` tree and chunk lists can be deep or cyclic).
- **Text is not UTF-8.** Legacy paths/labels are likely Mac Roman (the check is an open item); never assume a path exists or that an extension or 4cc matches the content.
- **Prove it.** Every crash fix comes with a small synthetic regression test that panicked before the fix. The truncation/corruption/fuzz-style tests must stay green.
- **Tests are the gate.** `cargo test -p ivc`, checked against `ivc-file-research/samples/` via `ivc-file-research/crates/ivc/tests/samples.rs`; add synthetic tests as well.
- **Evidence standard.** Never promote a field from UNKNOWN/PARTIAL to CONFIRMED without citing sample bytes (file, offset, observed values). Every format finding updates `ivc-file-research/docs/ivc-format.md` in the same change.
- **Clean-room.** Implement from observation and the spec; never copy iView/Expression/Phase One code or assets.
- **Never break wasm.** `ivc` is pure byte parsing and must stay compilable for `wasm32-unknown-unknown`. Status: the target is NOT installed here (`rustup target list --installed` shows only `aarch64-apple-darwin`), so `cargo check -p ivc --target wasm32-unknown-unknown` has not been run and no wasm status is claimed.

## 4. Before you finish a task

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

- Update `ivc-file-research/docs/ivc-format.md` for any new finding (with evidence).
- Append a terse entry to `log/devlog.md` (PLANNED, local and gitignored once the repo is under git): what landed, what's still open. Sessions end abruptly; the devlog plus a green tree is how the next agent picks up.
- Keep the tree building at every step.

## 5. Picking work

Take the UNKNOWN summary in `ivc-file-research/docs/ivc-format.md` §6. Current items: `FSM!` folder-tree internals, tag table (+0x1a6..0x330), Mac Roman encoding check, PICT pixmap decode, `PREF` / `CMRS` / `TOTL` payloads.

## 6. Parallel agents

Use your own target dir (`CARGO_TARGET_DIR=target/agent-<name>`) to avoid the Cargo build lock and edit only files you own. The root `Cargo.toml` is shared: make small, surgical edits and re-read before editing. Keep every `Cargo.toml` valid at all times: write to a temp file outside the crate directories, then `mv` it into place. If someone else's in-progress edit breaks the build, wait and retry; don't fix their files. Delete `target/agent-*` dirs of finished agents.
