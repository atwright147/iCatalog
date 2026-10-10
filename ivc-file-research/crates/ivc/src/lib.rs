//! Parser for iView / iView MediaPro / Expression Media / Phase One Media Pro
//! catalog files (`.ivc`, `.mpcatalog`). See `docs/ivc-format.md` for the format.
//!
//! The parser borrows from the input buffer and never panics on malformed input:
//! every out-of-range access yields an [`Error`].

use std::fmt;

/// Size of the fixed part of an item record (everything before the embedded PICT).
pub const ITEM_HEAD_LEN: usize = 0x400;
/// Chunk header size: 4-char type + `u32be` length (length excludes the header).
pub const CHUNK_HEADER_LEN: usize = 8;
/// Trailer size for `025i`: 4-byte magic + `u32be` chunk-sequence start offset.
pub const TRAILER_LEN_025: usize = 8;
/// Trailer size for `030i`: 4-byte magic + `u64be` chunk-sequence start offset.
pub const TRAILER_LEN_030: usize = 12;

/// Chunk types observed in the samples.
pub const KNOWN_CHUNKS: [&[u8; 4]; 16] = [
    b"FSM!", b"CELL", b"BLDO", b"USF3", b"CMRS", b"CSTT", b"TOTL", b"FONT", b"PREF", b"SVar",
    b"MSOP", b"MSVI", b"IMPF", b"MBWd", b"OpMs", b"VsMs",
];

/// Parse errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Input ended before `what` could be read.
    Truncated { what: &'static str, offset: usize },
    /// Magic after the item count is not `025i` / `030i`.
    BadMagic([u8; 4]),
    /// Trailer magic differs from header magic.
    TrailerMismatch { header: [u8; 4], trailer: [u8; 4] },
    /// A structural field holds an impossible value.
    Invalid { what: &'static str, offset: usize },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Truncated { what, offset } => {
                write!(f, "truncated input reading {what} at offset {offset:#x}")
            }
            Error::BadMagic(m) => write!(f, "unrecognised catalog magic {m:02x?}"),
            Error::TrailerMismatch { header, trailer } => {
                write!(
                    f,
                    "trailer magic {trailer:02x?} != header magic {header:02x?}"
                )
            }
            Error::Invalid { what, offset } => {
                write!(f, "invalid {what} at offset {offset:#x}")
            }
        }
    }
}

impl std::error::Error for Error {}

/// Result alias.
pub type Result<T> = std::result::Result<T, Error>;

/// File header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    /// Number of item records (`u32be` at offset 0).
    pub item_count: u32,
    /// `025i` (iView/Expression) or `030i` (Phase One).
    pub magic: [u8; 4],
}

/// File trailer (last 8 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Trailer {
    pub magic: [u8; 4],
    /// Offset of the first chunk (== end of the last item record).
    pub chunk_start: u64,
}

/// Top-level chunk (`type` + payload; length excludes the 8-byte header).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chunk<'a> {
    pub kind: [u8; 4],
    /// Absolute offset of the chunk header.
    pub offset: usize,
    pub payload: &'a [u8],
}

impl Chunk<'_> {
    /// Whether the chunk type was seen in the sample set.
    pub fn is_known(&self) -> bool {
        KNOWN_CHUNKS.iter().any(|k| **k == self.kind)
    }
}

/// A sized sub-block after the PICT of an item: `u32be size` (includes itself),
/// 4-char type (`TRCK`, `©swr`, ...), payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TailBlock<'a> {
    pub kind: [u8; 4],
    pub payload: &'a [u8],
}

/// One item record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item<'a> {
    /// Absolute offset of the record.
    pub offset: usize,
    /// Total record length in bytes.
    pub len: usize,
    /// Full native path (Pascal string at +0x000; Mac colon or Windows form).
    pub path: String,
    /// File name (Pascal string at +0x100).
    pub name: String,
    /// Format label at +0x14c (`JPEG`, `TIFF`, `GIFf`, ...).
    pub format: [u8; 4],
    /// `u32be` at +0x160 (equals the on-disk file size in samples).
    pub file_size: u32,
    /// `u32be` at +0x164.
    pub width: u32,
    /// `u32be` at +0x168.
    pub height: u32,
    /// Colour space label at +0x398 (`RGB `, `GRAY`, `INDX`, `RGB8`).
    pub color_space: [u8; 4],
    /// Embedded QuickDraw PICT thumbnail (starts at +0x400).
    pub pict: &'a [u8],
    /// JPEG stream inside the PICT, when the PICT is QuickTime-`jpeg` compressed.
    pub jpeg: Option<&'a [u8]>,
    /// Sized blocks following the PICT.
    pub tail_blocks: Vec<TailBlock<'a>>,
    /// IPTC-style datasets (`1c rec ds len16 value`) following the PICT, if any.
    pub iptc: Vec<IptcRecord<'a>>,
}

/// One IPTC-style dataset found after an item's PICT.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IptcRecord<'a> {
    pub record: u8,
    pub dataset: u8,
    pub value: &'a [u8],
}

/// A parsed catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalog<'a> {
    pub header: Header,
    pub items: Vec<Item<'a>>,
    pub chunks: Vec<Chunk<'a>>,
    pub trailer: Trailer,
}

fn slice<'a>(data: &'a [u8], start: usize, len: usize, what: &'static str) -> Result<&'a [u8]> {
    let end = start.checked_add(len).ok_or(Error::Truncated {
        what,
        offset: start,
    })?;
    data.get(start..end).ok_or(Error::Truncated {
        what,
        offset: start,
    })
}

fn be32(data: &[u8], at: usize, what: &'static str) -> Result<u32> {
    let b = slice(data, at, 4, what)?;
    Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

fn tag(data: &[u8], at: usize, what: &'static str) -> Result<[u8; 4]> {
    let b = slice(data, at, 4, what)?;
    Ok([b[0], b[1], b[2], b[3]])
}

fn pstring(head: &[u8], at: usize, max: usize, what: &'static str) -> Result<String> {
    let len = usize::from(*head.get(at).ok_or(Error::Truncated { what, offset: at })?);
    if len > max {
        return Err(Error::Invalid { what, offset: at });
    }
    let bytes = slice(head, at + 1, len, what)?;
    Ok(String::from_utf8_lossy(bytes).into_owned())
}

fn is_tag(t: &[u8; 4]) -> bool {
    t.iter().all(|b| (0x20..0x7f).contains(b) || *b == 0xa9)
}

fn usize_of(v: u32, what: &'static str, offset: usize) -> Result<usize> {
    usize::try_from(v).map_err(|_| Error::Invalid { what, offset })
}

/// Parse one item record starting at `start`. `limit` is the end of the area
/// usable by items (start of the trailer).
fn parse_item(data: &[u8], start: usize, limit: usize) -> Result<Item<'_>> {
    let area = data.get(..limit).ok_or(Error::Truncated {
        what: "item area",
        offset: limit,
    })?;
    let head = slice(area, start, ITEM_HEAD_LEN, "item head")?;
    let path = pstring(head, 0, 255, "item path")?;
    let name = pstring(head, 0x100, 0x4b, "item name")?;
    let format = tag(head, 0x14c, "format")?;
    let file_size = be32(head, 0x160, "file size")?;
    let width = be32(head, 0x164, "width")?;
    let height = be32(head, 0x168, "height")?;
    let color_space = tag(head, 0x398, "color space")?;

    let pict_len = usize_of(
        be32(head, 0x3f4, "pict length")?,
        "pict length",
        start + 0x3f4,
    )?;
    if pict_len < 16 {
        return Err(Error::Invalid {
            what: "pict length",
            offset: start + 0x3f4,
        });
    }
    let pict_start = start + ITEM_HEAD_LEN;
    let pict = slice(area, pict_start, pict_len, "pict")?;

    // QuickTime-compressed JPEG inside the PICT: codec tag at +0x484, data
    // length at +0x4ac, stream at +0x4d6 (all relative to record start).
    let jpeg = jpeg_in_pict(area, start, pict_start, pict_len);

    let mut pos = pict_start + pict_len;
    let mut tail_blocks = Vec::new();
    let mut iptc = Vec::new();
    loop {
        match area.get(pos) {
            Some(0x1c) | Some(0x1d) => {
                let hdr = slice(area, pos, 5, "iptc header")?;
                let vlen = usize::from(u16::from_be_bytes([hdr[3], hdr[4]]));
                let value = slice(area, pos + 5, vlen, "iptc value")?;
                iptc.push(IptcRecord {
                    record: hdr[1],
                    dataset: hdr[2],
                    value,
                });
                pos += 5 + vlen;
            }
            Some(0) if pos + CHUNK_HEADER_LEN <= limit => {
                let size = usize_of(be32(area, pos, "tail block size")?, "tail block size", pos)?;
                if size < CHUNK_HEADER_LEN {
                    break;
                }
                let kind = tag(area, pos + 4, "tail block type")?;
                let payload = slice(
                    area,
                    pos + CHUNK_HEADER_LEN,
                    size - CHUNK_HEADER_LEN,
                    "tail block",
                )?;
                tail_blocks.push(TailBlock { kind, payload });
                pos += size;
            }
            _ => break,
        }
    }

    Ok(Item {
        offset: start,
        len: pos - start,
        path,
        name,
        format,
        file_size,
        width,
        height,
        color_space,
        pict,
        jpeg,
        tail_blocks,
        iptc,
    })
}

fn jpeg_in_pict(area: &[u8], start: usize, pict_start: usize, pict_len: usize) -> Option<&[u8]> {
    let codec = area.get(start + 0x484..start + 0x488)?;
    if codec != b"jpeg" {
        return None;
    }
    let len_bytes = area.get(start + 0x4ac..start + 0x4b0)?;
    let len = usize::try_from(u32::from_be_bytes([
        len_bytes[0],
        len_bytes[1],
        len_bytes[2],
        len_bytes[3],
    ]))
    .ok()?;
    let from = start + 0x4d6;
    let to = from.checked_add(len)?;
    if from < pict_start || to > pict_start + pict_len {
        return None;
    }
    let mut jpeg = area.get(from..to)?;
    // Quirk (Expression2-s02 ArrowR.tif): the stored length can be one byte short
    // and cut the final `FF D9` marker in half.
    if jpeg.last() == Some(&0xff) && area.get(to) == Some(&0xd9) && to < pict_start + pict_len {
        jpeg = area.get(from..to + 1)?;
    }
    if jpeg.starts_with(&[0xff, 0xd8, 0xff]) {
        Some(jpeg)
    } else {
        None
    }
}

/// Parse a whole catalog file.
pub fn parse(data: &[u8]) -> Result<Catalog<'_>> {
    let count = be32(data, 0, "item count")?;
    let magic = tag(data, 4, "magic")?;
    if &magic != b"025i" && &magic != b"030i" {
        return Err(Error::BadMagic(magic));
    }
    let header = Header {
        item_count: count,
        magic,
    };

    let trailer_len = if &magic == b"030i" {
        TRAILER_LEN_030
    } else {
        TRAILER_LEN_025
    };
    let limit = data
        .len()
        .checked_sub(trailer_len)
        .filter(|l| *l >= 8)
        .ok_or(Error::Truncated {
            what: "trailer",
            offset: 0,
        })?;
    let trailer_magic = tag(data, limit, "trailer magic")?;
    let chunk_start = if trailer_len == TRAILER_LEN_030 {
        let b = slice(data, limit + 4, 8, "trailer chunk offset")?;
        u64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
    } else {
        u64::from(be32(data, limit + 4, "trailer chunk offset")?)
    };
    let trailer = Trailer {
        magic: trailer_magic,
        chunk_start,
    };
    if trailer.magic != magic {
        return Err(Error::TrailerMismatch {
            header: magic,
            trailer: trailer.magic,
        });
    }

    let mut items = Vec::new();
    let mut pos = 8usize;
    for _ in 0..count {
        let item = parse_item(data, pos, limit)?;
        pos += item.len;
        items.push(item);
    }

    let mut chunks = Vec::new();
    while pos < limit {
        let kind = tag(data, pos, "chunk type")?;
        if !is_tag(&kind) {
            return Err(Error::Invalid {
                what: "chunk type",
                offset: pos,
            });
        }
        let len = usize_of(
            be32(data, pos + 4, "chunk length")?,
            "chunk length",
            pos + 4,
        )?;
        let body = pos + CHUNK_HEADER_LEN;
        let end = body.checked_add(len).ok_or(Error::Invalid {
            what: "chunk length",
            offset: pos + 4,
        })?;
        if end > limit {
            return Err(Error::Truncated {
                what: "chunk payload",
                offset: body,
            });
        }
        chunks.push(Chunk {
            kind,
            offset: pos,
            payload: slice(data, body, len, "chunk payload")?,
        });
        pos = end;
    }

    Ok(Catalog {
        header,
        items,
        chunks,
        trailer,
    })
}
