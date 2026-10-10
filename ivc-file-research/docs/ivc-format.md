# iView / Expression Media / Phase One catalog format (`.ivc`, `.mpcatalog`)

Clean-room notes derived only from the 16 files in `samples/`. All integers are big-endian.
Offsets are absolute file offsets unless prefixed `+` (relative to the start of an item record).
Status tags: **CONFIRMED** (holds in every sample), **PARTIAL**, **UNKNOWN** (byte range given, semantics not claimed).
Reference implementation: `crates/ivc` (the tests walk all 16 samples).

## 1. File layout

```
0x00  u32  item_count N                       CONFIRMED (all 16 samples)
0x04  4cc  magic: "025i" (iView, Expression) | "030i" (Phase One .mpcatalog)
0x08  item record x N (variable length, contiguous)
      chunk sequence (until trailer)
EOF-8 (025i) / EOF-12 (030i): trailer
```

Empty catalogs (N = 0): the chunk sequence starts at 0x08 (e.g. `iView157-01`, `Expression1-s01`).

### Trailer — CONFIRMED
* `025i` files: last 8 bytes = magic `025i` + `u32` = **absolute offset of the first chunk** (= end of the last item record).
  `iView157-01`: `025i` at 0x42c, value 8. `Expression1-s02` (8320 B): `025i` at 0x2078, value 0x10fa = offset of `FSM!`. `Expression2-s01`: 0x9266.
  The value is therefore *not* always 8 (earlier hypothesis refuted by `Expression1-s02`).
* `030i` files: last **12** bytes = magic `030i` + `u64` chunk-start offset (`PhaseOneMediaPro1`: `00 00 00 00 00 00 42 3c`, `FSM!` at 0x423c; `PhaseOneMediaPro2`: 0x3aac).

## 2. Chunks — CONFIRMED framing
`4-char type` + `u32 length` (length **excludes** the 8-byte header) + payload. Chunks run back-to-back from the trailer offset up to the trailer; every sample walks exactly to the trailer start.
Proof: `iView157-01`: `TOTL` at 0x08 len 0x14, `PREF` at 0x24 len 0x400, trailer at 0x42c.

| Type | Seen in | Payload (len) | Notes |
|---|---|---|---|
| `FSM!` | all except `iView157-0x` | variable | Folder tree: nested `fldr` records; strings such as `<root>`, UTF-16 names (`Folders`), `\\vmware-host\shared folders…` (`Expression1-s02` 0x10fa, len 251). Inner layout PARTIAL: first `fldr` = `fldr 00 00 00 32 …`, name `3c 72 6f 6f 74 3e` ("`<root>`" with 1-byte length at +0x22?); subsequent entries begin with a leading `u32` before `fldr`, so naive nested chunk-walking misaligns. Inner structure UNKNOWN. |
| `CELL` | 7 | 12 (`Expression1-s02` 0x11fd) – 60 | `00 00 00 08 00 00 00 01 00 00 00 01`; UNKNOWN. |
| `BLDO` | 8 | 12 | UNKNOWN. |
| `USF3` | 8 | 2 | `01 03`; UNKNOWN. |
| `CMRS` | 8 | 493 | UNKNOWN (looks like a list of settings). |
| `CSTT` | 8 | 4 | `00 00 00 01`. |
| `TOTL` | 15 | 20 (0x14) | totals/stats: `00 00 00 01 …`; first u32 equals item count in `Expression1-s02` (1). Rest UNKNOWN. |
| `FONT` | 13 | 2064 | font table, UNKNOWN (starts `00 0b 0b`). |
| `PREF` | 15 | 1024 (0x400) | preferences blob; starts `ff ff ff ff 00 00 00 00 00 01 00 00 e8 03 00 fa`; UNKNOWN. |
| `SVar` | 13 | 4 | `00 00 01 f4` (500) in all files that have it (not in `iView157-*`). Last chunk in most files. |
| `MSOP`, `MSVI` | 4 (`.mpcatalog`/Pro 3.x) | small | UNKNOWN. |
| `IMPF` | 5 | 116 | `iViewMedia*` / `IVM264`, UNKNOWN. |
| `MBWd`, `OpMs`, `VsMs` | `iViewMedia1.3-Win-s01` (and some) | 8 / 5 / 5 | appear after `SVar` in that file; UNKNOWN. |

Chunk order differs by version (see §5). `INDX` and `TRCK` are **not** chunks (see §3.5).

## 3. Item record
Layout (offsets `+x`, verified on `Expression1-s02` item @0x08, `Expression2-s01` ×4, `iView157-02` ×5, `iViewPro26-s02` ×5, `PhaseOneMediaPro1/2`).

### 3.1 Head (fixed, `+0x000..+0x400`) — CONFIRMED length
| Range | Field | Verdict |
|---|---|---|
| +0x000 | Pascal string (`u8 len` + bytes) in a 256-byte zero padded buffer: full native path. Mac colon form (`OSX:Library:Application Support:Apple:iChat Icons:Fruit:Green Apple.gif`, `Macintosh HD:Users:tyler:Desktop:_K6A9098.jpg`, len byte 0x47 @0x08 in `iViewPro26-s02`) or Windows (`C:\DOCUME~1\…`, `\\vmware-host\shared folders\Desktop\memory.jpg`, len 0x2f @0x08 in `Expression1-s02`). Windows paths use 8.3 short names (`BLUEHI~1.JPG`). | **CONFIRMED**: leading byte *is* a u8 length (the earlier "inconsistent" evidence was the length byte being printable ASCII, e.g. `G`, `:`, `/`, `<`). Text encoding (Mac Roman?) UNKNOWN; parser uses lossy UTF-8. |
| +0x100 | Pascal string: file name (`0a "memory.jpg"` @0x108 in `Expression1-s02`; `0f "Green Apple.gif"`; `Blue hills.jpg` in `Expression2-s01` shows the *display* name differs from the path's 8.3 name). Max seen well below 0x4c. | CONFIRMED |
| +0x14c | 4cc format: `JPEG`, `TIFF`, `GIFf` | CONFIRMED |
| +0x150..+0x160 | UNKNOWN (zero in most samples; creator/type codes?) | UNKNOWN |
| +0x160 | u32 file size (`Expression2-s01` Winter.jpg = 105542; `iView157-02` Green Apple.gif = 2159) | CONFIRMED |
| +0x164 | u32 width px (800 / 48 / 3840) | CONFIRMED |
| +0x168 | u32 height px (600 / 48 / 5356) | CONFIRMED |
| +0x16c | u32 `0x60` (JPEG/TIFF) or `0x48` (GIF); likely resolution — UNKNOWN | UNKNOWN |
| +0x170..+0x1a6 | small flags/ints (`0f 0f 0f ff` @+0x171 GIF; `02 04 08 ff` JPEG) | UNKNOWN |
| +0x18a (Mac) / +0x1a6.. | Mac volume name pstring (`06 "Mac107"`), timestamps (`e4 ef 7a df …`; Mac epoch 1904 seconds probable) | PARTIAL / UNKNOWN |
| +0x1a6..+0x330 | tagged region: second Mac colon path as pstring (`4f "Mac107:Library:…"` @+0x253), UTF-16BE file name (`00 47 00 72 …` @+0x2a5 in `PhaseOneMediaPro1`), tag `00 12` + `u16 len` + **POSIX path** (`"Library/Application Support/Apple/iChat Icons/Fruit/Green Apple.gif"` @+0x2dd; `/vmware-host/sHARED FOLDERS/Desktop/memory.jpg` in `Expression1-s02`), `00 13 00 01 2f`, `ff ff` @+0x328. Individual tag IDs not decoded. | UNKNOWN structure, content visible |
| +0x340..+0x398 | three u32 timestamps (e.g. `e4 f0 ac 35 e4 66 7f 43 e4 66 7f 43` @+0x340) + counters | UNKNOWN |
| +0x398 | 4cc colour space: `RGB `, `GRAY`, `INDX` (GIF in 030i/Pro), `RGB8` (GIF in `iView157-02`) | CONFIRMED |
| +0x39c..+0x3f4 | `00 01 00 01 03 "GIF"`/`00 01 00 03 0c "Photo - JPEG"` pstring (kind label) and zeros | PARTIAL |
| +0x3f4 | **u32 PICT length** | CONFIRMED |
| +0x3f8..+0x400 | UNKNOWN (`@+0x3f0` = `0x124` in 030i-era items = size of the later `TRCK` block) | UNKNOWN |

### 3.2 Thumbnail = QuickDraw PICT v2 at `+0x400`, length from `+0x3f4` — CONFIRMED
`Expression2-s01` item 0: PICT length 0x24de, `0x08+0x400+0x24de = 0x28e6` = start of item 1. Item 1 start also matches the strings-scan offset. PICT begins with `u16 picSize`, 8-byte frame rect, `00 11 02 ff 0c 00 ff fe` (version op), … and ends with opcode `00 ff` (the `00 00 00 ff` seen in hexdumps is its last 4 bytes).
* Non-classic items: a QuickTime `0x8200` compressed-image opcode carries an ImageDescription: `u32 0x56` @+0x480, codec `jpeg` @+0x484, `appl` vendor @+0x49b, compressor name pstring `Photo - JPEG` @+0x4bb, **u32 JPEG data length @+0x4ac**, **JFIF stream starts @+0x4d6** (`ff d8 ff e0 00 10 'JFIF'`, e.g. 0x4de in `Expression1-s02`, length 3048 → ends FFD9 at 0x10c4).
  Quirk: in `Expression2-s02` (ArrowR.tif) the stored length (2473) is 1 byte short; the JPEG really is 2474 bytes (ends `…3f ff d9`). Parser extends by one byte when it ends in `FF` followed by `D9`.
* After the JPEG the PICT contains the fallback text op ("QuickTime™ and a Photo - JPEG decompressor are needed to see this picture.").
* `iView157-02` (classic Mac 1.5.7): PICT is an uncompressed pixmap with a colour table (`RGB8`; no JPEG anywhere in the file); the JPEG-magic test therefore applies to every sample except `iView157-02`.

### 3.3 Tail after the PICT — PARTIAL (framing CONFIRMED)
Zero or more of, until the next item (first byte non-zero = path length) or chunk:
* **sized blocks**: `u32 size` (includes itself) + 4-byte type + payload. Seen: `TRCK` (size 0x124, payload `05 "Video"`, … `03 "GIF"` — track/kind info, `iView157-02` @0x1100-4, `PhaseOneMediaPro1` @0xbbc+4); `©swr` (`a9 73 77 72`: `u16 len, u16 0, text` e.g. "Adobe Photoshop CC 2019 (Macintosh)", `Expression1-s02` @0x10ca size 0x30); `©mak`, `©mod`, and 15 `ex??` 16-byte blocks (EXIF-style, `ex 82 9a` = ExposureTime…) in `PhaseOneMediaPro2` 0x39a0..0x3aab. Type bytes are not always ASCII.
* **IPTC-style datasets**: `1c|1d  record  dataset  u16 len  value` (e.g. `1c 02 50 00 25 "1992 ACCUSOFT INC, ALL RIGHTS RESERVE"` in `Expression2-s02`; `1d 02 50 00 0e "Tyler Thorsted"` in `PhaseOneMediaPro2`). First byte is `1c`, but `1d` also observed on the first dataset — UNKNOWN why.
* Plain items (`Expression2-s01`) have no tail.
Item end is determined by walking these; every sample's last item ends exactly at the trailer's chunk offset.

### 3.4 Dimensions / metadata exposed by `crates/ivc`
path, name, format, file size, width, height, colour space, PICT bytes, JPEG slice, tail blocks, IPTC records.

### 3.5 `INDX` / `TRCK` / `jpeg` strings
`INDX` is the colour-space label at +0x398 (not a chunk). `TRCK` is a sized tail block. A naive "chunk" regex finds them but they are item-internal.

## 4. Verified item offsets
`Expression2-s01`: 0x08, 0x28e6, 0x3efe, 0x5d8e (chunks @0x9266). `PhaseOneMediaPro1`: 0x08, 0xce4, 0x1ad6, 0x2716, 0x3476 (chunks @0x423c). `iView157-02`: 0x08, 0x1220, 0x247e, 0x3598, 0x477a (chunks @0x595e). `Expression1-s02`: 0x08 (chunks @0x10fa). `Expression2-s02`: 0x08 (chunks @0xeb6). `PhaseOneMediaPro2`: 0x08 (chunks @0x3aac).

## 5. Version differences
| Sample set | Magic | Chunk sequence |
|---|---|---|
| iView 1.5.7 classic Mac (`iView157-*`) | 025i | `TOTL, [CELL], PREF` — no `FSM!`, no `SVar`, no `FONT`; trailer value 8 / item end. Items: raw-pixmap PICT, `RGB8`, `TRCK` block. |
| iView MediaPro 1.3 / 2.x / IVM 2.6.4 (Win) | 025i | `FSM!, TOTL, [IMPF], FONT, PREF, SVar, …` (`iViewMedia1.3`: + `MBWd, OpMs, VsMs`) |
| iView Pro 2.6 / 3.0.2 / 3.1.1 | 025i | `FSM!, [CELL], [BLDO, USF3, CMRS, CSTT], TOTL, FONT, PREF, SVar` (CELL/BLDO… from 3.0.2) |
| Expression Media 1 / 2 | 025i | as Pro 3.x |
| Phase One Media Pro 1 / 2 (`.mpcatalog`) | **030i** | as Pro 3.x plus `MSOP`/`MSVI`; trailer has a `u64` offset (12-byte trailer); richer tail metadata (©/ex blocks, IPTC). |

## 6. UNKNOWN summary / next steps
Inner structure of `FSM!` (`fldr` tree), `CELL`, `BLDO`, `USF3`, `CMRS`, `FONT`, `PREF`, `TOTL`; item bytes +0x150..+0x160, +0x16c..+0x398 (tag table, timestamps, flags), +0x3f8..+0x400; text encoding of path strings; why the PICT JPEG length is off by one in `Expression2-s02`.

## 7. References
* **Sample provenance:** samples courtesy of Tyler Thorsted's PRONOM_Research project: https://github.com/thorsted/PRONOM_Research/tree/main/Submissions/iView/Samples (16 files, found via a blog post on `https://preservation.tylerthorsted.com`, Feb 2026, iView entry). They are licensed **CC0 1.0** (public domain). The samples are committed; `ivc-file-research/samples/fetch.sh` can optionally re-fetch them from pinned commit `2c26686a0577028783b2f24977cdaf78e6ca39fb` and verify via `SHA256SUMS`.
* **Official legacy format manual (archived):** https://web.archive.org/web/20060313112803/http://www.iview-multimedia.com/bin/manuals/file_formats.pdf
* **Archived original iView binaries** (behaviour-study candidates; clean-room rules still apply): https://web.archive.org/web/20060401070901/http://www.iview-multimedia.com/bin/
* **PRONOM `fmt/647`** (The National Archives): https://www.nationalarchives.gov.uk/PRONOM/fmt/647 — matches magic at offset 4 and a 16-byte EOF pattern.
