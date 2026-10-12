//! Embedded-JPEG preview extraction from RAW containers (TIFF-based, RAF, CR3).
//!
//! All parsing is bounds-checked: hostile input yields an error, never a panic.

use crate::{probe, MediaError, Result};

const MAX_PREVIEW: usize = 64 * 1024 * 1024;
const MAX_IFD_DEPTH: usize = 8;
const MAX_IFDS: usize = 64;
const MAX_ENTRIES: usize = 4096;
const MAX_CANDIDATES: usize = 64;
const MAX_BOX_DEPTH: usize = 4;
const MAX_BOXES: usize = 4096;
const RAF_MAGIC: &[u8; 16] = b"FUJIFILMCCD-RAW ";

fn err(msg: &str) -> MediaError {
    MediaError::Decode(msg.to_owned())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Tiff,
    Raf,
    Cr3,
}

fn sniff(bytes: &[u8]) -> Option<Kind> {
    let head = bytes.get(..4)?;
    if head == b"II*\0" || head == b"MM\0*" {
        return Some(Kind::Tiff);
    }
    if bytes.get(..16) == Some(&RAF_MAGIC[..]) {
        return Some(Kind::Raf);
    }
    if bytes.get(4..8) == Some(&b"ftyp"[..]) && bytes.get(8..12) == Some(&b"crx "[..]) {
        return Some(Kind::Cr3);
    }
    None
}

/// Content-based sniff: true if `bytes` looks like a supported RAW container.
pub fn is_raw(bytes: &[u8]) -> bool {
    sniff(bytes).is_some()
}

/// Return the largest embedded JPEG preview found in a RAW container.
pub fn raw_preview(bytes: &[u8]) -> Result<Vec<u8>> {
    let kind = sniff(bytes).ok_or_else(|| err("not a supported RAW container"))?;
    let mut cands: Vec<(usize, usize)> = Vec::new();
    match kind {
        Kind::Tiff => tiff_candidates(bytes, &mut cands),
        Kind::Raf => {}
        Kind::Cr3 => cr3_candidates(bytes, 0, bytes.len(), 0, &mut 0, &mut cands),
    }
    // Largest first.
    cands.sort_by_key(|&(_, len)| std::cmp::Reverse(len));
    cands.dedup();
    for &(off, len) in &cands {
        if let Some(j) = valid_jpeg(bytes, off, len) {
            return Ok(j);
        }
    }
    // Fallback: bounded JPEG segment walk. A TIFF that declared candidates but
    // none were usable is treated as corrupt rather than scanned.
    if kind == Kind::Tiff && !cands.is_empty() {
        return Err(err("declared JPEG preview is unusable"));
    }
    let mut scanned = scan_jpegs(bytes, 0);
    scanned.sort_by_key(|&(_, len)| std::cmp::Reverse(len));
    for &(off, len) in &scanned {
        if let Some(j) = valid_jpeg(bytes, off, len) {
            return Ok(j);
        }
    }
    Err(err("no usable embedded JPEG preview"))
}

fn valid_jpeg(bytes: &[u8], off: usize, len: usize) -> Option<Vec<u8>> {
    if !(4..=MAX_PREVIEW).contains(&len) {
        return None;
    }
    let end = off.checked_add(len)?;
    let slice = bytes.get(off..end)?;
    if slice.get(..3)? != [0xFF, 0xD8, 0xFF] {
        return None;
    }
    let info = probe(slice).ok()?;
    if !info.format.eq_ignore_ascii_case("jpeg") {
        return None;
    }
    Some(slice.to_vec())
}

/// Find the end (exclusive) of the JPEG starting at `start`, by walking segments.
fn jpeg_end(bytes: &[u8], start: usize) -> Option<usize> {
    if bytes.get(start..start.checked_add(3)?)? != [0xFF, 0xD8, 0xFF] {
        return None;
    }
    let mut pos = start.checked_add(2)?;
    loop {
        // Expect a marker.
        if *bytes.get(pos)? != 0xFF {
            return None;
        }
        let mut m = *bytes.get(pos.checked_add(1)?)?;
        pos = pos.checked_add(1)?;
        while m == 0xFF {
            pos = pos.checked_add(1)?;
            m = *bytes.get(pos)?;
        }
        pos = pos.checked_add(1)?;
        match m {
            0xD9 => return Some(pos),
            0x00 | 0x01 | 0xD0..=0xD7 | 0xD8 => continue,
            _ => {}
        }
        let len = usize::from(u16::from_be_bytes([
            *bytes.get(pos)?,
            *bytes.get(pos.checked_add(1)?)?,
        ]));
        if len < 2 {
            return None;
        }
        pos = pos.checked_add(len)?;
        if m == 0xDA {
            // Entropy-coded data: skip to the next real marker.
            loop {
                let b = *bytes.get(pos)?;
                if b == 0xFF {
                    let n = *bytes.get(pos.checked_add(1)?)?;
                    if n != 0x00 && !(0xD0..=0xD7).contains(&n) && n != 0xFF {
                        break;
                    }
                    if n == 0xFF {
                        pos = pos.checked_add(1)?;
                        continue;
                    }
                    pos = pos.checked_add(2)?;
                } else {
                    pos = pos.checked_add(1)?;
                }
            }
        }
    }
}

/// Bounded scan for JPEGs; skips past each found JPEG so nested previews are ignored.
fn scan_jpegs(bytes: &[u8], from: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = from;
    while out.len() < MAX_CANDIDATES {
        let Some(tail) = bytes.get(i..) else { break };
        let Some(rel) = tail.windows(3).position(|w| w == [0xFF, 0xD8, 0xFF]) else {
            break;
        };
        let start = i.saturating_add(rel);
        match jpeg_end(bytes, start) {
            Some(end) if end > start => {
                out.push((start, end - start));
                i = end;
            }
            _ => i = start.saturating_add(1),
        }
    }
    out
}

struct Tiff<'a> {
    b: &'a [u8],
    le: bool,
}

impl Tiff<'_> {
    fn u16(&self, o: usize) -> Option<u16> {
        let a = self.b.get(o..o.checked_add(2)?)?;
        let a = [*a.first()?, *a.get(1)?];
        Some(if self.le {
            u16::from_le_bytes(a)
        } else {
            u16::from_be_bytes(a)
        })
    }
    fn u32(&self, o: usize) -> Option<u32> {
        let a = self.b.get(o..o.checked_add(4)?)?;
        let a = [*a.first()?, *a.get(1)?, *a.get(2)?, *a.get(3)?];
        Some(if self.le {
            u32::from_le_bytes(a)
        } else {
            u32::from_be_bytes(a)
        })
    }
    /// SHORT/LONG values of an entry (capped).
    fn values(&self, entry: usize) -> Option<Vec<u32>> {
        let ty = self.u16(entry.checked_add(2)?)?;
        let count = usize::try_from(self.u32(entry.checked_add(4)?)?).ok()?;
        let size = match ty {
            3 => 2usize,
            4 => 4usize,
            _ => return None,
        };
        if count == 0 || count > MAX_ENTRIES {
            return None;
        }
        let total = count.checked_mul(size)?;
        let data = if total <= 4 {
            entry.checked_add(8)?
        } else {
            usize::try_from(self.u32(entry.checked_add(8)?)?).ok()?
        };
        let mut v = Vec::with_capacity(count);
        for k in 0..count {
            let o = data.checked_add(k.checked_mul(size)?)?;
            v.push(if size == 2 {
                u32::from(self.u16(o)?)
            } else {
                self.u32(o)?
            });
        }
        Some(v)
    }
}

fn tiff_candidates(bytes: &[u8], out: &mut Vec<(usize, usize)>) {
    let le = bytes.get(..2) == Some(&b"II"[..]);
    let t = Tiff { b: bytes, le };
    let Some(first) = t.u32(4).and_then(|v| usize::try_from(v).ok()) else {
        return;
    };
    let mut queue: Vec<(usize, usize)> = vec![(first, 0)];
    let mut visited: Vec<usize> = Vec::new();
    while let Some((ifd, depth)) = queue.pop() {
        if depth > MAX_IFD_DEPTH
            || visited.len() >= MAX_IFDS
            || ifd == 0
            || visited.contains(&ifd)
            || out.len() >= MAX_CANDIDATES
        {
            continue;
        }
        visited.push(ifd);
        let Some(n) = t.u16(ifd) else { continue };
        let n = usize::from(n).min(MAX_ENTRIES);
        let (mut jo, mut jl, mut comp) = (None, None, None);
        let (mut so, mut sc): (Option<Vec<u32>>, Option<Vec<u32>>) = (None, None);
        for i in 0..n {
            let Some(e) = i
                .checked_mul(12)
                .and_then(|x| x.checked_add(2))
                .and_then(|x| x.checked_add(ifd))
            else {
                break;
            };
            let Some(tag) = t.u16(e) else { break };
            match tag {
                0x0201 => jo = t.values(e).and_then(|v| v.first().copied()),
                0x0202 => jl = t.values(e).and_then(|v| v.first().copied()),
                0x0103 => comp = t.values(e).and_then(|v| v.first().copied()),
                0x0111 => so = t.values(e),
                0x0117 => sc = t.values(e),
                0x014A => {
                    if let Some(v) = t.values(e) {
                        for off in v.into_iter().take(16) {
                            if let Ok(o) = usize::try_from(off) {
                                queue.push((o, depth + 1));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        if let (Some(o), Some(l)) = (jo, jl) {
            push(out, o, l);
        }
        if let (Some(6 | 7), Some(so), Some(sc)) = (comp, so, sc) {
            if let Some(&o) = so.first() {
                let total = sc.iter().try_fold(0u32, |a, &c| a.checked_add(c));
                if let Some(l) = total {
                    push(out, o, l);
                }
            }
        }
        let next = n
            .checked_mul(12)
            .and_then(|x| x.checked_add(2))
            .and_then(|x| x.checked_add(ifd))
            .and_then(|e| t.u32(e))
            .and_then(|v| usize::try_from(v).ok());
        if let Some(nx) = next {
            queue.push((nx, depth + 1));
        }
    }
}

/// True pixel dimensions of the full-resolution image in a RAW container, when
/// the container declares them (TIFF-based RAW: the sub-image whose
/// NewSubfileType bit 0 is clear, i.e. not a reduced-resolution preview).
/// Returns None when it cannot be determined (e.g. CR3/RAF, or tags absent).
pub fn raw_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if sniff(bytes)? != Kind::Tiff {
        return None;
    }
    let t = Tiff {
        b: bytes,
        le: bytes.get(..2) == Some(&b"II"[..]),
    };
    let first = usize::try_from(t.u32(4)?).ok()?;
    let mut queue: Vec<(usize, usize)> = vec![(first, 0)];
    let mut visited: Vec<usize> = Vec::new();
    let mut best: Option<(u32, u32)> = None;
    let mut exif_ifds: Vec<usize> = Vec::new();
    while let Some((ifd, depth)) = queue.pop() {
        if depth > MAX_IFD_DEPTH || visited.len() >= MAX_IFDS || ifd == 0 || visited.contains(&ifd)
        {
            continue;
        }
        visited.push(ifd);
        let Some(n) = t.u16(ifd) else { continue };
        let n = usize::from(n).min(MAX_ENTRIES);
        let (mut subfile, mut w, mut h) = (None, None, None);
        for i in 0..n {
            let Some(e) = i
                .checked_mul(12)
                .and_then(|x| x.checked_add(2))
                .and_then(|x| x.checked_add(ifd))
            else {
                break;
            };
            let Some(tag) = t.u16(e) else { break };
            match tag {
                0x00FE => subfile = t.values(e).and_then(|v| v.first().copied()),
                0x0100 => w = t.values(e).and_then(|v| v.first().copied()),
                0x0101 => h = t.values(e).and_then(|v| v.first().copied()),
                0x8769 => {
                    if let Some(o) = t
                        .values(e)
                        .and_then(|v| v.first().copied())
                        .and_then(|o| usize::try_from(o).ok())
                    {
                        if exif_ifds.len() < 16 {
                            exif_ifds.push(o);
                        }
                    }
                }
                0x014A => {
                    if let Some(v) = t.values(e) {
                        for off in v.into_iter().take(16) {
                            if let Ok(o) = usize::try_from(off) {
                                queue.push((o, depth + 1));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        if let (Some(w), Some(h)) = (w, h) {
            let full = subfile.is_none_or(|s| s & 1 == 0);
            if full && w > 0 && h > 0 {
                let area = |d: (u32, u32)| u64::from(d.0).saturating_mul(u64::from(d.1));
                if best.is_none_or(|b| area((w, h)) > area(b)) {
                    best = Some((w, h));
                }
            }
        }
        let next = n
            .checked_mul(12)
            .and_then(|x| x.checked_add(2))
            .and_then(|x| x.checked_add(ifd))
            .and_then(|e| t.u32(e))
            .and_then(|v| usize::try_from(v).ok());
        if let Some(nx) = next {
            queue.push((nx, depth + 1));
        }
    }
    if best.is_some() {
        return best;
    }
    // Fallback: ExifIFD PixelXDimension / PixelYDimension.
    for ifd in exif_ifds {
        let Some(n) = t.u16(ifd) else { continue };
        let n = usize::from(n).min(MAX_ENTRIES);
        let (mut w, mut h) = (None, None);
        for i in 0..n {
            let Some(e) = i
                .checked_mul(12)
                .and_then(|x| x.checked_add(2))
                .and_then(|x| x.checked_add(ifd))
            else {
                break;
            };
            let Some(tag) = t.u16(e) else { break };
            match tag {
                0xA002 => w = t.values(e).and_then(|v| v.first().copied()),
                0xA003 => h = t.values(e).and_then(|v| v.first().copied()),
                _ => {}
            }
        }
        if let (Some(w), Some(h)) = (w, h) {
            if w > 0 && h > 0 {
                return Some((w, h));
            }
        }
    }
    None
}

fn push(out: &mut Vec<(usize, usize)>, off: u32, len: u32) {
    if let (Ok(o), Ok(l)) = (usize::try_from(off), usize::try_from(len)) {
        out.push((o, l));
    }
}

fn cr3_candidates(
    bytes: &[u8],
    start: usize,
    end: usize,
    depth: usize,
    count: &mut usize,
    out: &mut Vec<(usize, usize)>,
) {
    if depth > MAX_BOX_DEPTH {
        return;
    }
    let end = end.min(bytes.len());
    let mut pos = start;
    while pos < end && *count < MAX_BOXES && out.len() < MAX_CANDIDATES {
        *count += 1;
        let Some(size32) = bytes
            .get(pos..pos.saturating_add(4))
            .and_then(|s| <[u8; 4]>::try_from(s).ok())
            .map(u32::from_be_bytes)
        else {
            return;
        };
        let Some(ty) = bytes.get(pos.saturating_add(4)..pos.saturating_add(8)) else {
            return;
        };
        let (hdr, size) = match size32 {
            0 => (8usize, end - pos),
            1 => {
                let Some(big) = bytes
                    .get(pos.saturating_add(8)..pos.saturating_add(16))
                    .and_then(|s| <[u8; 8]>::try_from(s).ok())
                    .map(u64::from_be_bytes)
                else {
                    return;
                };
                let Ok(big) = usize::try_from(big) else {
                    return;
                };
                (16usize, big)
            }
            n => (8usize, usize::try_from(n).unwrap_or(0)),
        };
        if size < hdr {
            return;
        }
        let Some(box_end) = pos.checked_add(size) else {
            return;
        };
        let box_end = box_end.min(end);
        let body = pos + hdr;
        match ty {
            b"moov" => cr3_candidates(bytes, body, box_end, depth + 1, count, out),
            b"uuid" => cr3_candidates(
                bytes,
                body.saturating_add(16),
                box_end,
                depth + 1,
                count,
                out,
            ),
            b"PRVW" | b"THMB" => {
                // The preview JPEG follows a box-local header whose size varies
                // by camera; search a bounded prefix of the payload for its SOI.
                const SCAN_WINDOW: usize = 1024 * 1024;
                let window_end = body.saturating_add(SCAN_WINDOW).min(box_end);
                if let Some(w) = bytes.get(body..window_end) {
                    if let Some(rel) = w.windows(3).position(|x| x == [0xFF, 0xD8, 0xFF]) {
                        let s = body.saturating_add(rel);
                        if let Some(e) = jpeg_end(bytes, s) {
                            out.push((s, e - s));
                        }
                    }
                }
            }
            _ => {}
        }
        if box_end <= pos {
            return;
        }
        pos = box_end;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use image::{codecs::jpeg::JpegEncoder, ExtendedColorType};

    fn jpeg(w: u32, h: u32) -> Vec<u8> {
        let px: Vec<u8> = (0..w * h * 3).map(|i| (i % 251) as u8).collect();
        let mut out = Vec::new();
        JpegEncoder::new(&mut out)
            .encode(&px, w, h, ExtendedColorType::Rgb8)
            .unwrap();
        out
    }

    fn w16(le: bool, v: u16) -> [u8; 2] {
        if le {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        }
    }
    fn w32(le: bool, v: u32) -> [u8; 4] {
        if le {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        }
    }

    /// TIFF with a chain of IFDs, each pointing at one JPEG via 0x0201/0x0202.
    fn tiff(le: bool, jpegs: &[Vec<u8>], bad_offset: bool) -> Vec<u8> {
        let ifd_size = 2 + 2 * 12 + 4;
        let mut out = Vec::new();
        out.extend_from_slice(if le { b"II*\0" } else { b"MM\0*" });
        out.extend_from_slice(&w32(le, 8));
        let mut data_off = 8 + ifd_size * jpegs.len();
        for (i, j) in jpegs.iter().enumerate() {
            out.extend_from_slice(&w16(le, 2));
            let off = if bad_offset {
                0x7FFF_0000
            } else {
                data_off as u32
            };
            for (tag, val) in [(0x0201u16, off), (0x0202, j.len() as u32)] {
                out.extend_from_slice(&w16(le, tag));
                out.extend_from_slice(&w16(le, 4));
                out.extend_from_slice(&w32(le, 1));
                out.extend_from_slice(&w32(le, val));
            }
            let next = if i + 1 < jpegs.len() {
                8 + ifd_size * (i + 1)
            } else {
                0
            };
            out.extend_from_slice(&w32(le, next as u32));
            data_off += j.len();
        }
        for j in jpegs {
            out.extend_from_slice(j);
        }
        out
    }

    fn cr3(j: &[u8]) -> Vec<u8> {
        let mut prvw = vec![0u8; 20];
        prvw.extend_from_slice(j);
        let mut prvw_box = ((prvw.len() + 8) as u32).to_be_bytes().to_vec();
        prvw_box.extend_from_slice(b"PRVW");
        prvw_box.extend_from_slice(&prvw);
        let mut uuid = vec![0u8; 16];
        uuid.extend_from_slice(&prvw_box);
        let mut uuid_box = ((uuid.len() + 8) as u32).to_be_bytes().to_vec();
        uuid_box.extend_from_slice(b"uuid");
        uuid_box.extend_from_slice(&uuid);
        let mut moov = ((uuid_box.len() + 8) as u32).to_be_bytes().to_vec();
        moov.extend_from_slice(b"moov");
        moov.extend_from_slice(&uuid_box);
        let mut out = 20u32.to_be_bytes().to_vec();
        out.extend_from_slice(b"ftypcrx \0\0\0\x01crx isom");
        out.extend_from_slice(&moov);
        out
    }

    #[test]
    fn tiff_le_and_be() {
        let j = jpeg(16, 16);
        for le in [true, false] {
            let f = tiff(le, std::slice::from_ref(&j), false);
            assert!(is_raw(&f));
            assert_eq!(raw_preview(&f).unwrap(), j);
        }
    }

    #[test]
    fn largest_candidate_wins() {
        let small = jpeg(8, 8);
        let big = jpeg(64, 64);
        let f = tiff(true, &[small, big.clone()], false);
        assert_eq!(raw_preview(&f).unwrap(), big);
    }

    #[test]
    fn raf_preview() {
        let j = jpeg(16, 16);
        let mut f = RAF_MAGIC.to_vec();
        f.extend_from_slice(&[0u8; 40]);
        f.extend_from_slice(&j);
        f.extend_from_slice(&[0u8; 10]);
        assert!(is_raw(&f));
        assert_eq!(raw_preview(&f).unwrap(), j);
    }

    #[test]
    fn cr3_preview() {
        let j = jpeg(16, 16);
        let f = cr3(&j);
        assert!(is_raw(&f));
        assert_eq!(raw_preview(&f).unwrap(), j);
    }

    #[test]
    fn non_raw_rejected() {
        let mut png = Vec::new();
        image::DynamicImage::new_rgb8(2, 2)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        assert!(!is_raw(&png));
        assert!(raw_preview(&png).is_err());
        assert!(raw_preview(&[1, 2, 3, 4, 5, 6, 7, 8, 9]).is_err());
        assert!(!is_raw(&[]));
    }

    #[test]
    fn bigtiff_rejected() {
        assert!(!is_raw(b"II+\0\x08\0\0\0"));
        assert!(raw_preview(b"MM\0+\0\x08\0\0").is_err());
    }

    #[test]
    fn out_of_range_offset_errors() {
        let f = tiff(true, &[jpeg(8, 8)], true);
        assert!(raw_preview(&f).is_err());
    }

    #[test]
    fn nested_exif_jpeg_not_truncated() {
        // Outer JPEG whose APP1 embeds a smaller JPEG (with its own EOI).
        let inner = jpeg(8, 8);
        let outer = jpeg(32, 32);
        let mut app1 = b"Exif\0\0".to_vec();
        app1.extend_from_slice(&inner);
        let seg_len = (app1.len() + 2) as u16;
        let mut v = vec![0xFF, 0xD8, 0xFF, 0xE1];
        v.extend_from_slice(&seg_len.to_be_bytes());
        v.extend_from_slice(&app1);
        v.extend_from_slice(outer.get(2..).unwrap());
        let mut f = RAF_MAGIC.to_vec();
        f.extend_from_slice(&v);
        assert_eq!(raw_preview(&f).unwrap(), v);
    }

    /// LE TIFF; each IFD is a list of (tag, value) LONG entries; IFD i chains
    /// to i+1; `sub` = (parent ifd index, child ifd index) adds a SubIFD tag.
    fn tiff_ifds(ifds: &[Vec<(u16, u32)>], sub: Option<(usize, usize)>) -> Vec<u8> {
        let size = |n: usize| 2 + 12 * n + 4;
        let mut lists: Vec<Vec<(u16, u32)>> = ifds.to_vec();
        let mut offs = Vec::new();
        let mut pos = 8usize;
        // Provisional layout (sub entry adds one entry to parent).
        if let Some((p, _)) = sub {
            lists[p].push((0x014A, 0));
        }
        for l in &lists {
            offs.push(pos);
            pos += size(l.len());
        }
        if let Some((p, c)) = sub {
            let off = offs[c] as u32;
            lists[p].last_mut().unwrap().1 = off;
        }
        let mut out = b"II*\0".to_vec();
        out.extend_from_slice(&8u32.to_le_bytes());
        for (i, l) in lists.iter().enumerate() {
            out.extend_from_slice(&(l.len() as u16).to_le_bytes());
            let mut l = l.clone();
            l.sort_by_key(|e| e.0);
            for (tag, v) in l {
                out.extend_from_slice(&tag.to_le_bytes());
                out.extend_from_slice(&4u16.to_le_bytes());
                out.extend_from_slice(&1u32.to_le_bytes());
                out.extend_from_slice(&v.to_le_bytes());
            }
            let next = if sub.is_none() && i + 1 < lists.len() {
                offs[i + 1] as u32
            } else {
                0
            };
            out.extend_from_slice(&next.to_le_bytes());
        }
        out
    }

    #[test]
    fn dimensions_from_subifd() {
        let f = tiff_ifds(
            &[
                vec![(254, 1), (256, 1024), (257, 683)],
                vec![(254, 0), (256, 6048), (257, 4024)],
            ],
            Some((0, 1)),
        );
        assert_eq!(raw_dimensions(&f), Some((6048, 4024)));
    }

    #[test]
    fn dimensions_largest_candidate() {
        let f = tiff_ifds(
            &[
                vec![(254, 0), (256, 100), (257, 100)],
                vec![(254, 0), (256, 6048), (257, 4024)],
            ],
            None,
        );
        assert_eq!(raw_dimensions(&f), Some((6048, 4024)));
    }

    #[test]
    fn dimensions_reduced_only_is_none() {
        let f = tiff_ifds(&[vec![(254, 1), (256, 1024), (257, 683)]], None);
        assert_eq!(raw_dimensions(&f), None);
    }

    /// Reduced-only IFD0 with an ExifIFD (IFD index 1, via 0x8769 -> its offset).
    fn tiff_with_exif(exif: Vec<(u16, u32)>) -> Vec<u8> {
        // IFD0 has 3 entries + ExifIFD pointer = 4 entries.
        let ifd0_size = 2 + 12 * 4 + 4;
        let exif_off = (8 + ifd0_size) as u32;
        let mut f = tiff_ifds(
            &[vec![(254, 1), (256, 1024), (257, 683), (0x8769, exif_off)]],
            None,
        );
        f.extend_from_slice(&(exif.len() as u16).to_le_bytes());
        for (tag, v) in exif {
            f.extend_from_slice(&tag.to_le_bytes());
            f.extend_from_slice(&4u16.to_le_bytes());
            f.extend_from_slice(&1u32.to_le_bytes());
            f.extend_from_slice(&v.to_le_bytes());
        }
        f.extend_from_slice(&0u32.to_le_bytes());
        f
    }

    #[test]
    fn dimensions_from_exif_ifd() {
        let f = tiff_with_exif(vec![(0xA002, 6000), (0xA003, 4000)]);
        assert_eq!(raw_dimensions(&f), Some((6000, 4000)));
    }

    #[test]
    fn dimensions_exif_ifd_unusable_is_none() {
        assert_eq!(raw_dimensions(&tiff_with_exif(vec![(0xA002, 6000)])), None);
        assert_eq!(
            raw_dimensions(&tiff_with_exif(vec![(0xA002, 0), (0xA003, 0)])),
            None
        );
        assert_eq!(raw_dimensions(&tiff_with_exif(vec![])), None);
    }

    #[test]
    fn dimensions_exif_ifd_truncation_safe() {
        let f = tiff_with_exif(vec![(0xA002, 6000), (0xA003, 4000)]);
        for n in 0..f.len() {
            let _ = raw_dimensions(f.get(..n).unwrap());
        }
    }

    #[test]
    fn dimensions_plain_tiff_uses_ifd0() {
        let f = tiff_ifds(&[vec![(256, 20), (257, 10)]], None);
        assert_eq!(raw_dimensions(&f), Some((20, 10)));
    }

    #[test]
    fn dimensions_cr3_raf_none_and_truncation_safe() {
        let j = jpeg(16, 16);
        assert_eq!(raw_dimensions(&cr3(&j)), None);
        let mut raf = RAF_MAGIC.to_vec();
        raf.extend_from_slice(&j);
        assert_eq!(raw_dimensions(&raf), None);
        let f = tiff_ifds(
            &[
                vec![(254, 1), (256, 1024), (257, 683)],
                vec![(254, 0), (256, 6048), (257, 4024)],
            ],
            Some((0, 1)),
        );
        for n in 0..f.len() {
            let _ = raw_dimensions(f.get(..n).unwrap());
        }
    }

    #[test]
    fn truncation_and_mutation_never_panic() {
        let j = jpeg(16, 16);
        let files = [
            tiff(true, &[jpeg(8, 8), j.clone()], false),
            tiff(false, std::slice::from_ref(&j), false),
            cr3(&j),
            {
                let mut f = RAF_MAGIC.to_vec();
                f.extend_from_slice(&j);
                f
            },
        ];
        for f in &files {
            for n in 0..f.len() {
                let t = f.get(..n).unwrap();
                let _ = is_raw(t);
                let _ = raw_preview(t);
            }
            for i in 0..f.len().min(120) {
                for b in [0x00u8, 0xFF, 0x7F] {
                    let mut m = f.clone();
                    if let Some(x) = m.get_mut(i) {
                        *x = b;
                    }
                    let _ = raw_preview(&m);
                }
            }
        }
    }
}
