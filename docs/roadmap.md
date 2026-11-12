# iCatalog roadmap

Purpose: what is built, what is in progress, and what comes next, app-side. Crate map and layering live in `docs/architecture.md`; numbered decisions in `docs/decisions.md`; the format study in `ivc-file-research/AGENTS.md`. Status vocabulary: **Decided** (owner order), **Proposed** (breakdown), **Open** (unresolved). The exit criteria for every milestone are the workspace gate: `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`.

App-side crates that exist today: `icatalog-catalog`, `icatalog-media`, `icatalog-engine`, `icatalog-sync`, `icatalog-cli`, `icatalog-ui` (plus `ivc`, in `ivc-file-research/`).

## 1. Priority rule

**Decided:** format knowledge first, then the data model, then the interfaces. App-side order: schema, engine + CLI, sync, egui (desktop, then wasm); M4 was pulled ahead of M3 by owner request (D15). Crash-safety fixes outrank feature work (golden rule).

## 2. Done

- **M0 housekeeping** — repo under git; root `.gitignore` (`target/`, `log/`, `*.sqlite`, `*.icatalog`); `crates/*` workspace members; `log/devlog.md` (local, gitignored).
- **Development agents (D59)** — five project-scoped Codex roles in `.codex/agents/`: legacy-format research, catalogue/sync, native UI, release/deployment and independent read-only review. [Scope and usage](agents.md); roles run on demand and do not change feature priorities.
- **M1 schema** — `icatalog-catalog`: schema v1-v8 with migrations, the model + CRUD for every table (including `source` with device provenance and source-rooted folders, D21), `AssetQuery`/`CatalogStats`; round-trip, FK-cascade, constraint, raw-bytes, migration and never-panic tests. The one open piece — the `ivc` import mapping — is M2 work, not schema work.

## 3. In progress

- **M2 engine + CLI** — *landed:* the `icatalog-engine` command registry + `dispatch_guarded` panic guard; the `icatalog` commands `init`, `open`, `import` (D13, now source-rooted folders with `--kind`, D21), `list`, `find`, `status`, `update` (D14, offline sources are not marked missing, D21), `edit` (D16), `previews` generation/storage actions (D20/D44) and manual `collection` actions (D25, including `list`/`find --collection`); the layer test; `icatalog-media` raster ingest (`image`), EXIF (`kamadak-exif`) and grid thumbnails; per-reason skip reporting in `import`/`update`; the adversarial-params fuzz test (`crates/icatalog-engine/tests/adversarial.rs`); non-UTF-8 OS arguments are handled without a CLI panic (D61). *Open:* a `source`/`folder` command surface, legacy `.ivc`/`.mpcatalog` import (deprioritised) and the PICT pixel decoder (`ivc` work, tracked in `ivc-file-research/AGENTS.md`).
- **M4 egui (desktop)** — *landed:* `icatalog-ui`, a thin native egui shell (D15) that browses assets, shows a stored thumbnail + metadata, edits rating/label/keywords, and dispatches `init`/`import`/`update`/`edit`/`collection` as engine commands. The left panel has a source-rooted folder tree grouped under collapsible device headers (D21; selecting a device shows all its sources' assets), the centre a grid/list view with search, and the right a details panel. Selecting a folder includes all descendants, including collapsed branches, and search narrows the subtree (D43). Manual Collections can be created through the sidebar's New Collection modal (D26), show catalogue-wide asset counts (D30), and are selected in the sidebar, with grid/list drag-and-drop (D27), multiple asset selection and tri-state collection membership checkboxes in the context submenu (D29). Shift-click ranges and Ctrl/⌘-click toggles combine selected collections without duplicate assets (D28). *Open:* the wasm build (trunk + wasm-bindgen; the target is not installed and no wasm status is claimed).
- **M3 sync** — *landed (D20/D44):* protocol v2 server-authoritative catalogue sync, direct `clone` setup without a manual copy (D45), desktop Connect/Clone and explicit Sync dialogs with remembered local addresses (D46), revision-checked rating/label/keyword and Collection uploads, an embedded replica baseline, idle no-ops, reported per-entity conflicts with draft backups and idempotent lost-response retry. Snapshot POSTs are retired. Server data writes advance generation transactionally; consistent downloads are checked for integrity, foreign keys and header/database agreement before backup and replacement. Collection UIDs distinguish concurrent creations. Explicit `previews status|generate|external|embed` manages oriented 1920-pixel JPEGs with embedded/external storage; referenced previews are fetched using GET only and verified/cached by SHA-256. Replica media writes are blocked; annotation/Collection editing stays available. NAS packaging includes static archives (D58) and Docker/Compose (D60). *Open:* incremental catalogue pulls, per-field merges, automatic startup sync, preview viewing/garbage collection, NAS release publication, container publication and service installation.
- **Media breadth (D8/D17)** — *landed:* EXIF-oriented thumbnails with display fallback for older catalogues (D31), RAW embedded-preview thumbnails, RAW container format + true dimensions, and XMP packet storage with seed-on-first-ingest of rating/label/keywords — with no new dependencies (D17, D31). *Open:* sidecar `.xmp`; PSD; HEIC/AVIF (metadata only); video metadata + frame (FilmCraft); GUI full-size preview viewing (the per-catalogue embedded/external 1920-pixel JPEG store and CLI generation/fetch exist, D20/D44). No demosaic, by design.

Smart Collections (D35) are implemented in the catalogue, engine/CLI and desktop: typed AND rules for format, capture dates, camera/lens, rating/label, keyword and filename/path text; live membership/counts; mixed collection unions/search; create and replace-rule commands; modal creation and right-click rule editing. Schema v6 preserves existing manual collections. OR groups, nested rules and manual-to-smart conversion are not included.

Smart Collection previews (D37) show a live catalogue-wide match count and readable AND summary in Create/Edit Rules before saving. The same read-only preview is available through `collection preview-smart`. Background previews debounce typing, discard stale results and report invalid rules/read errors; long summaries scroll while fields and buttons remain visible in 800×600 windows. No schema or sync change.

All collections can be renamed (D36) through `collection rename` or the sidebar's right-click Rename… modal. Renaming preserves ids, membership, rules, counts and active selections; failures retain the entered name for retry. No schema change.

Smart Collection names and rules can be edited together (D38) through the shared editor and optional `set-rules --name`. Double-clicking a Smart Collection selects it and opens Edit Rules; Save/Enter commits both changes atomically. Invalid input/failures retain the draft; pending commands block opening another editor. Normal Collections open Rename on double-click (D41). macOS double-click timing follows the system preference with a 500 ms fallback (D39); slower-click and batched-input regressions supplement the original fast-click tests.

Smart Collection selection stays stable between double-clicks (D40): only plain-click deselection of an already selected smart row is deferred until the double-click interval expires. New selections and modifier gestures remain immediate. D41 extends the same behavior to normal Collections, whose double-click opens Rename with the current name selected. Tests assert collection and asset selection between clicks and cover cancellation of obsolete toggles, rename/Enter/cancel/reopen and busy guards.

D23 hardening is implemented: atomic engine command dispatch, reported UI refresh/startup errors, background import/update/edit/collection, visible-row rendering and bounded thumbnail caching. Regression coverage includes partial writes, panic/commit rollback and 10,000-row viewport bounds; `task test:scale` adds opt-in generated-image and sync workloads. Remaining scale work includes asset-query paging, asynchronous visible-thumbnail decoding, per-file progress/cancellation and representative RAW-library benchmarks.

D24 UI checks exist: real-widget interaction/persistence tests in the workspace gate, opt-in GPU screenshot comparisons (including populated/multiple collections, creation modal, context menu, drop target and long-name collection counts, D25-D30), and a separately invoked native macOS menu/picker-cancellation/quit smoke test. Regression coverage includes membership-cache invalidation and busy/read-error handling, count/name spacing and rectangle selection while scrolling. D96 resolved six Linux screenshots that differed from reviewed baselines by at most two channel levels, using measured Linux-only tolerances of 1.0 or 3.0 while retaining 0.6 elsewhere, zero failed pixels and unchanged baselines. The hosted workspace, visual and Windows hard-link checks passed. Manual self-hosted execution and broader native interaction/platform coverage remain open.

The native Update smoke timeout is resolved: the test uses a unique temporary app identity with guarded PID targeting to avoid confusing another running iCatalog, and waits for foreground/menu readiness and picker dismissal. Three consecutive final local smoke runs passed Update persistence, Import cancellation and Command-Q; the workspace gate passed. Automation stage logs now survive failures/timeouts. Self-hosted native workflow execution remains unverified.

List rows (D32) show thumbnails plus filename/rating/captured and modified dates/filesize; document/camera/lens/focal length; dimensions/colour/ICC profile. List uses the browsing width, an optional Details inspector, and horizontal scrolling in narrow windows. Import/update capture supported source colour information, and Update fills older entries without regenerating unchanged thumbnails. RAW source colour remains unavailable rather than inferred from its preview. Tests cover layout, missing metadata, edit refresh, profile removal/backfill/reopen and narrow/list visual baselines.

Collections fill the remaining sidebar height (D49), with scrolling for long lists and retained per-Collection member counts. The standalone asset-total footer and separator are removed. Checks cover short/tall windows, later-row selection after scrolling and populated-list screenshots.

Folders navigation fills the available sidebar width (D42), including All/Unassigned, device headers and indented folder names. Disclosure arrows remain separate; long names truncate with hover text. Headless checks cover right-edge selection, nesting, collapse/reopen, both themes and display scales; screenshots cover device headers in both themes.

Long folder trees scroll independently below fixed All/Unassigned controls (D62), using at most 45% of the remaining sidebar height. Collections remains reachable and retains its own scroll area. A headless 800×600 check scrolls to late rows in both lists and selects them.

Details inspector and app dialogs have roomier local text layouts (D47): inset inspector content, larger line/section gaps, wrapping annotation controls and vertical scrolling; wider Collection/Smart rules/Connect/Sync dialog margins and field/button gaps. Rename/Edit Rules place Name beside its field, with Name aligned to rule inputs in Edit Rules. Smart rule fields/actions still fit at 800×600. Tests cover both themes/display scales, scrolled keyword editing and long sync diagnostics; screenshots include metadata and the scrolled inspector. Sidebar, grid/List and native OS picker spacing are unchanged.

Edit Rules has a reusable Rules fieldset around the instructions and ten rule inputs (D48), with accessible grouping and a legend in the border. Name, match preview and actions stay outside. Input alignment and 800×600 fit are preserved; both themes/scales and grouped rule editing are covered. Creation/Rename layouts are unchanged.

App dialogs have top-right X close controls through a shared heading component (D50). They use existing Cancel/Close semantics, discard unsaved drafts and disable closing during pending saves/transfers. Checks cover placement, pointer/accessibility activation, error/result dismissal and busy guards; screenshots cover each modal.

Details inspector paths truncate in the middle through the shared `middle_ellipsis::label` component (D51), retaining both ends on one line and the full path on hover and for accessibility. Measured-width, Unicode, narrow-layout and Grid/List inspector checks cover both themes/display scales; screenshots cover long paths in both themes.

Details file sizes use the same shared human-readable formatter as List (D52), with binary units, one decimal place above bytes and explicit unknown values. Unit-boundary/hostile-value tests and inspector theme/scale checks cover the display; stored sizes and CLI output are unchanged.

Details exposure seconds are concise through shared `formatting::exposure_time` (D55): trim to two decimal places, preserving two significant digits for exposures below 0.01s and removing redundant zeros. Invalid values remain explicit; tests cover extremes, real inspector display and preserved stored precision.

Smart rules also support an optional inclusive Maximum Rating (D57) in Create/Edit Rules and CLI `--max-rating`. Either bound can stand alone; 0 includes unrated assets, equal bounds select an exact rating, and inverted ranges fail validation without changing stored rules/name. Schema v8 preserves existing rules/UIDs/revisions/baselines; sync extends Collection records while retaining old baseline decoding and safe refusal by older servers. Tests cover migration/rollback, shared queries/previews, CLI, sync/conflicts and both-theme 1×/1.5×/2× editing. Ten rule rows use 4-point gaps to retain 800×600 fit.

Ratings use reusable five-star components in List, Details and Create/Edit Smart Collection rules (D56/D57). List stays read-only; editors support pointer/keyboard selection and clearing, preserving unrated assets and the distinction between no bound and an explicit zero bound. Existing edit/collection commands and busy guards persist changes; component, persistence, preview and theme/scale checks cover the shared controls.

Details and List share regional date/time display (D53/D54) with numeric regional date order, padded day/month, full years and an at separator before the system-format time: Foundation on macOS, native user formatters on Windows, and locale-environment settings on Linux. Modified uses the system local zone; Captured preserves its stored camera-local wall time. Invalid values remain explicit, the display cache is bounded and refreshed, and UI fixtures use deterministic regional settings. Tests cover regions/clocks, offsets/DST semantics, invalid input and native macOS output. Stored dates, search/sort and CLI behavior are unchanged; native Windows/Linux runtime verification remains platform-specific.

Desktop legibility (D33) uses larger interface/caption text and List metadata, stronger normal/secondary/selection contrast in both themes, hinted glyphs without fractional variants, and roomier rows. Native display scaling is preserved. Layout checks cover both themes at 1×/1.5×/2×; light and Retina List screenshots supplement existing baselines.

Desktop release automation (D34) exists for published releases: Windows x64/ARM64 ZIPs, macOS ARM64 `.app` + CLI archive, and Linux x86-64 Debian `.deb`, Fedora `.rpm` and Arch `.pkg.tar.zst`. Manual dispatch produces workflow artifacts only by default; `release_tag` builds the tag's application with current workflow packaging and uploads to its existing release. Hosted v0.1.0 recovery passed all four builds, all three Linux installation checks and final upload. All six original assets and SHA256SUMS are published and their digests verified. Arch constraints are guarded by a generated-metadata regression. macOS Intel/universal releases are no longer targeted. Developer ID signing/notarization and Windows publisher signing remain open. D58 adds two static NAS CLI archives and native CI jobs for future runs; native x86-64/ARM64 hosted checks passed in [run 43113311921](https://github.com/atwright147/iCatalog/actions/runs/43113311921); NAS release publication remains unverified.

New catalogue filenames prefer `.icatalog` (D89). Linux desktop packages and
the macOS bundle register that extension; Finder cold/running open events
switch the native desktop to a verified existing catalogue (D97). The Windows
ZIP includes an opt-in per-user registration helper for Open with and Default
apps; Windows retains the user's default-app choice. Existing `.sqlite` names
remain supported and are verified from their contents before migration.

Docker/Compose NAS packaging (D60) exists for x86-64/ARM64 static CLI builds. The scratch image uses a non-root numeric user and a minimal generated build context. Compose persists the catalogue directory, mounts media/rootfs read-only, injects init for stop-signal forwarding and defaults to loopback publication. `serve` refuses missing catalogues without SQLite creation; initialization/import remain explicit. Disposable runtime checks cover previews, permissions, clone/edit/Collection sync and persistence after server replacement; native CI also exercises the supplied Compose lifecycle. [Setup and maintenance](nas-docker.md). Native Docker/Compose checks passed on both architectures in [run 43113311871](https://github.com/atwright147/iCatalog/actions/runs/43113311871). Registry publication and a systemd installer remain unbuilt.

HTTPS and shared-token authentication (D98) protect native server and client
connections. Serve accepts a PEM certificate/key pair and optional token file;
clone/sync accept token and private-CA files, including desktop file selection.
Credential-bearing remote connections require HTTPS; literal-loopback HTTP supports
a local TLS reverse proxy. Certificate verification and redirects stay strict,
failed credentials preserve local work, and no secret enters catalogue snapshots
or URL preferences. Certificate issuance/renewal, user accounts and persistent
desktop credential storage are not included. The vendored HTTP dependency has
bounded rejected-upload handling and fallible TLS configuration.

## 4. Next


- **D20/D44 and D98 are implemented**, including Collection edits, HTTPS and shared-token authentication. Next sync work is automatic desktop startup behavior. See `docs/sync.md` for the shipped v2 protocol and first-sync upgrade behavior.
- **M3 sync, deployment half** — static NAS archives (D58) and Docker/Compose packaging (D60) are implemented. Native hosted archive and Docker/Compose checks passed on both architectures at `a26a708` on 2026-11-11. Remaining deployment work: registry/release publication and service installation.
- Then close the M2/M3 gaps above; `wasm32-unknown-unknown` install and the first wasm check join the gate once the target is available.

## 5. Open backlog

- Sync: automatic startup sync; incremental pulls; per-field conflict merging; Collection deletion/conversion; preview viewer/garbage collection; share-file mode vs a mandatory service.
- cargo-deny. `ATTRIBUTION.md` now records the patched HTTP dependency (D98); no third-party artwork is shipped.
- Per-crate `clippy.toml`.
- `xtask` (`layers`, `ci`).
- Install `wasm32-unknown-unknown` and run the first wasm check.

## 6. How priority is decided

The owner sets order (recorded in `docs/decisions.md` and `AGENTS.md`); crash-safety fixes outrank feature work (**Decided** golden rule). New decisions get a numbered entry in `docs/decisions.md`; this file then follows.
