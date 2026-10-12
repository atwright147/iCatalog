//! Media decode, metadata extraction and thumbnails (decision D8).
//! Bytes/seekable reader in, plain data out; depends on no other workspace crate.
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

mod raw;
pub use raw::{is_raw, raw_dimensions, raw_preview};
mod xmp;
pub use xmp::{parse_xmp, read_xmp, xmp_packet, XmpData};

use std::fmt;
use std::io::{BufRead, Cursor, Seek};

use exif::{In, Tag, Value};
use image::{ImageFormat, ImageReader, Limits};
use sha2::{Digest, Sha256};

/// Canonical grid thumbnail long edge, in pixels.
pub const GRID_LONG_EDGE: u32 = 256;
/// Thumbnail generator version.
pub const GENERATOR_VERSION: i64 = 1;

const MAX_DIM: u32 = 1_000_000;

/// Errors from media decoding and metadata extraction.
#[derive(Debug)]
pub enum MediaError {
    Decode(String),
    Exif(String),
    Unsupported(String),
}

impl fmt::Display for MediaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MediaError::Decode(m) => write!(f, "decode error: {m}"),
            MediaError::Exif(m) => write!(f, "exif error: {m}"),
            MediaError::Unsupported(m) => write!(f, "unsupported: {m}"),
        }
    }
}

impl std::error::Error for MediaError {}

impl From<image::ImageError> for MediaError {
    fn from(e: image::ImageError) -> Self {
        MediaError::Decode(e.to_string())
    }
}

impl From<exif::Error> for MediaError {
    fn from(e: exif::Error) -> Self {
        MediaError::Exif(e.to_string())
    }
}

impl From<std::io::Error> for MediaError {
    fn from(e: std::io::Error) -> Self {
        MediaError::Decode(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, MediaError>;

/// Basic image geometry/format, read from the header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageInfo {
    pub format: String,
    pub width: i64,
    pub height: i64,
}

/// Normalised EXIF-derived metadata (mirrors the `asset_meta` columns).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExifData {
    pub captured_local: Option<String>,
    pub tz_offset_min: Option<i64>,
    pub make: Option<String>,
    pub model: Option<String>,
    pub lens: Option<String>,
    pub focal_length_mm: Option<f64>,
    pub f_number: Option<f64>,
    pub exposure_s: Option<f64>,
    pub iso: Option<i64>,
    pub gps_lat: Option<f64>,
    pub gps_lon: Option<f64>,
    pub orientation: Option<i64>,
}

/// A generated grid thumbnail (encoded bytes + geometry).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thumbnail {
    pub format: String,
    pub width: i64,
    pub height: i64,
    pub orientation_applied: bool,
    pub bytes: Vec<u8>,
}

/// A decoded image as tightly-packed RGBA8 rows, ready for display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

fn format_name(f: ImageFormat) -> String {
    match f {
        ImageFormat::Jpeg => "jpeg",
        ImageFormat::Png => "png",
        ImageFormat::Gif => "gif",
        ImageFormat::WebP => "webp",
        ImageFormat::Bmp => "bmp",
        ImageFormat::Tiff => "tiff",
        ImageFormat::Tga => "tga",
        ImageFormat::Pnm => "pnm",
        other => other.extensions_str().first().copied().unwrap_or("unknown"),
    }
    .to_owned()
}

fn check_dims(w: u32, h: u32) -> Result<()> {
    if w == 0 || h == 0 || w > MAX_DIM || h > MAX_DIM {
        return Err(MediaError::Decode(format!("invalid dimensions {w}x{h}")));
    }
    Ok(())
}

/// Read format and dimensions from the image header.
pub fn probe(bytes: &[u8]) -> Result<ImageInfo> {
    let reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let format = reader
        .format()
        .ok_or_else(|| MediaError::Decode("unrecognised image format".to_owned()))?;
    let (w, h) = reader.into_dimensions()?;
    check_dims(w, h)?;
    Ok(ImageInfo {
        format: format_name(format),
        width: i64::from(w),
        height: i64::from(h),
    })
}

fn ascii(v: &Value) -> Option<String> {
    if let Value::Ascii(parts) = v {
        let s = String::from_utf8_lossy(parts.first()?).into_owned();
        let s = s.trim_matches(|c: char| c == '\0' || c.is_whitespace());
        if !s.is_empty() {
            return Some(s.to_owned());
        }
    }
    None
}

fn rational(v: &Value, idx: usize) -> Option<f64> {
    let r = match v {
        Value::Rational(r) => r.get(idx)?.to_f64(),
        Value::SRational(r) => r.get(idx)?.to_f64(),
        _ => return None,
    };
    r.is_finite().then_some(r)
}

/// `YYYY:MM:DD HH:MM:SS` -> `YYYY-MM-DDTHH:MM:SS`.
fn exif_datetime_to_iso(s: &str) -> Option<String> {
    let b = s.trim().as_bytes();
    if b.len() != 19 {
        return None;
    }
    for (i, c) in b.iter().enumerate() {
        let ok = match i {
            4 | 7 | 13 | 16 => *c == b':',
            10 => *c == b' ',
            _ => c.is_ascii_digit(),
        };
        if !ok {
            return None;
        }
    }
    let t = s.trim();
    Some(format!(
        "{}-{}-{}T{}",
        t.get(0..4)?,
        t.get(5..7)?,
        t.get(8..10)?,
        t.get(11..19)?
    ))
}

/// `+HH:MM` / `-HH:MM` -> minutes.
fn offset_to_minutes(s: &str) -> Option<i64> {
    let s = s.trim();
    let sign = match s.chars().next()? {
        '+' => 1,
        '-' => -1,
        _ => return None,
    };
    let rest = s.get(1..)?;
    let (h, m) = rest.split_once(':')?;
    let h: i64 = h.parse().ok()?;
    let m: i64 = m.parse().ok()?;
    if !(0..=14).contains(&h) || !(0..60).contains(&m) {
        return None;
    }
    Some(sign * (h * 60 + m))
}

fn gps_coord(v: &Value, reference: Option<String>, neg: char) -> Option<f64> {
    let d = rational(v, 0)?;
    let m = rational(v, 1).unwrap_or(0.0);
    let s = rational(v, 2).unwrap_or(0.0);
    let mut deg = d + m / 60.0 + s / 3600.0;
    if reference.is_some_and(|r| r.to_ascii_uppercase().starts_with(neg)) {
        deg = -deg;
    }
    deg.is_finite().then_some(deg)
}

/// Extract normalised EXIF metadata. A source without EXIF yields defaults.
pub fn read_exif<R: BufRead + Seek>(mut src: R) -> Result<ExifData> {
    let exif = match exif::Reader::new().read_from_container(&mut src) {
        Ok(e) => e,
        Err(exif::Error::NotFound(_)) => return Ok(ExifData::default()),
        Err(e) => return Err(MediaError::from(e)),
    };
    let get = |t: Tag| exif.get_field(t, In::PRIMARY).map(|f| &f.value);
    let text = |t: Tag| get(t).and_then(ascii);
    let num = |t: Tag| get(t).and_then(|v| rational(v, 0));
    let uint = |t: Tag| get(t).and_then(|v| v.get_uint(0)).map(i64::from);

    let gps_lat = get(Tag::GPSLatitude).and_then(|v| gps_coord(v, text(Tag::GPSLatitudeRef), 'S'));
    let gps_lon =
        get(Tag::GPSLongitude).and_then(|v| gps_coord(v, text(Tag::GPSLongitudeRef), 'W'));

    Ok(ExifData {
        captured_local: text(Tag::DateTimeOriginal).and_then(|s| exif_datetime_to_iso(&s)),
        tz_offset_min: text(Tag::OffsetTimeOriginal).and_then(|s| offset_to_minutes(&s)),
        make: text(Tag::Make),
        model: text(Tag::Model),
        lens: text(Tag::LensModel),
        focal_length_mm: num(Tag::FocalLength),
        f_number: num(Tag::FNumber),
        exposure_s: num(Tag::ExposureTime),
        iso: uint(Tag::PhotographicSensitivity).or_else(|| uint(Tag::ISOSpeed)),
        gps_lat,
        gps_lon,
        orientation: uint(Tag::Orientation),
    })
}

/// Decode `bytes` and produce a JPEG grid thumbnail with the given long edge.
pub fn thumbnail_jpeg(bytes: &[u8], long_edge: u32) -> Result<Thumbnail> {
    let long_edge = if long_edge == 0 {
        GRID_LONG_EDGE
    } else {
        long_edge
    };
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(50_000);
    limits.max_image_height = Some(50_000);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let img = reader.decode()?;
    let (w, h) = (img.width(), img.height());
    check_dims(w, h)?;
    let long = w.max(h);
    let target = long_edge.min(long);
    let scale = |d: u32| -> u32 {
        let v = (u64::from(d) * u64::from(target) + u64::from(long) / 2) / u64::from(long);
        u32::try_from(v).unwrap_or(target).max(1)
    };
    let (tw, th) = (scale(w), scale(h));
    let rgb = image::DynamicImage::ImageRgb8(img.to_rgb8());
    let small = image::imageops::resize(&rgb, tw, th, image::imageops::FilterType::Triangle);
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 80).encode_image(&small)?;
    Ok(Thumbnail {
        format: "jpeg".to_owned(),
        width: i64::from(tw),
        height: i64::from(th),
        orientation_applied: false,
        bytes: out,
    })
}

/// Decode `bytes` to RGBA8 for display, bounded by [`Limits`].
pub fn decode_rgba(bytes: &[u8]) -> Result<DecodedImage> {
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(50_000);
    limits.max_image_height = Some(50_000);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let img = reader.decode()?;
    let (w, h) = (img.width(), img.height());
    check_dims(w, h)?;
    let rgba = img.to_rgba8();
    Ok(DecodedImage {
        width: w,
        height: h,
        rgba: rgba.into_raw(),
    })
}

/// SHA-256 of `bytes` as `sha256:<lowercase hex>`.
pub fn content_hash(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut s = String::from("sha256:");
    for b in digest {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use image::{DynamicImage, ImageEncoder, RgbaImage};

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = RgbaImage::from_fn(w, h, |x, y| {
            image::Rgba([(x % 256) as u8, (y % 256) as u8, 7, 255])
        });
        let mut out = Vec::new();
        image::codecs::png::PngEncoder::new(&mut out)
            .write_image(img.as_raw(), w, h, image::ExtendedColorType::Rgba8)
            .unwrap();
        out
    }

    fn jpeg(w: u32, h: u32) -> Vec<u8> {
        let img = DynamicImage::ImageRgba8(RgbaImage::from_fn(w, h, |x, y| {
            image::Rgba([(x % 256) as u8, (y % 256) as u8, 7, 255])
        }))
        .to_rgb8();
        let mut out = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut out)
            .encode_image(&img)
            .unwrap();
        out
    }

    #[test]
    fn probe_png_and_jpeg() {
        let p = probe(&png(8, 4)).unwrap();
        assert_eq!((p.format.as_str(), p.width, p.height), ("png", 8, 4));
        let j = probe(&jpeg(8, 4)).unwrap();
        assert_eq!((j.format.as_str(), j.width, j.height), ("jpeg", 8, 4));
    }

    #[test]
    fn probe_garbage_is_decode_error() {
        assert!(matches!(probe(b"not an image"), Err(MediaError::Decode(_))));
    }

    #[test]
    fn thumbnail_resizes_and_keeps_aspect() {
        let t = thumbnail_jpeg(&jpeg(1000, 400), 256).unwrap();
        assert_eq!(t.format, "jpeg");
        assert!(!t.bytes.is_empty());
        assert!(!t.orientation_applied);
        let back = image::load_from_memory(&t.bytes).unwrap();
        assert_eq!(i64::from(back.width()), t.width);
        assert!(t.width <= 256 && t.height > 0 && t.width > 0);
        assert!((t.height - 102).abs() <= 1);
    }

    #[test]
    fn thumbnail_zero_edge_uses_default() {
        let t = thumbnail_jpeg(&png(1000, 400), 0).unwrap();
        assert_eq!(t.width, i64::from(GRID_LONG_EDGE));
    }

    #[test]
    fn thumbnail_does_not_upscale_and_rejects_garbage() {
        let t = thumbnail_jpeg(&png(8, 4), 256).unwrap();
        assert_eq!((t.width, t.height), (8, 4));
        assert!(thumbnail_jpeg(b"junk", 256).is_err());
    }

    #[test]
    fn decode_rgba_geometry_and_garbage() {
        let d = decode_rgba(&png(8, 4)).unwrap();
        assert_eq!((d.width, d.height), (8, 4));
        assert_eq!(d.rgba.len(), 8 * 4 * 4);
        assert!(decode_rgba(b"not an image").is_err());
    }

    #[test]
    fn hash_is_stable_and_formatted() {
        let a = content_hash(b"abc");
        assert_eq!(a, content_hash(b"abc"));
        assert_ne!(a, content_hash(b"abd"));
        assert!(a.starts_with("sha256:"));
        assert_eq!(a.len(), 71);
        assert_eq!(
            a,
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn exif_absent_is_default() {
        let d = read_exif(Cursor::new(jpeg(8, 4))).unwrap();
        assert_eq!(d, ExifData::default());
    }

    #[test]
    fn exif_hand_built_tiff() {
        let mut t: Vec<u8> = vec![b'I', b'I', 42, 0, 8, 0, 0, 0];
        t.extend_from_slice(&2u16.to_le_bytes());
        // Make, ASCII, count 6, offset 38
        t.extend_from_slice(&0x010Fu16.to_le_bytes());
        t.extend_from_slice(&2u16.to_le_bytes());
        t.extend_from_slice(&6u32.to_le_bytes());
        t.extend_from_slice(&38u32.to_le_bytes());
        // ExifIFD pointer -> 44
        t.extend_from_slice(&0x8769u16.to_le_bytes());
        t.extend_from_slice(&4u16.to_le_bytes());
        t.extend_from_slice(&1u32.to_le_bytes());
        t.extend_from_slice(&44u32.to_le_bytes());
        t.extend_from_slice(&0u32.to_le_bytes());
        t.extend_from_slice(b"Canon\0");
        t.extend_from_slice(&1u16.to_le_bytes());
        // DateTimeOriginal, ASCII, count 20, offset 62
        t.extend_from_slice(&0x9003u16.to_le_bytes());
        t.extend_from_slice(&2u16.to_le_bytes());
        t.extend_from_slice(&20u32.to_le_bytes());
        t.extend_from_slice(&62u32.to_le_bytes());
        t.extend_from_slice(&0u32.to_le_bytes());
        t.extend_from_slice(b"2020:02:03 04:05:06\0");
        let d = read_exif(Cursor::new(t)).unwrap();
        assert_eq!(d.make.as_deref(), Some("Canon"));
        assert_eq!(d.captured_local.as_deref(), Some("2020-02-03T04:05:06"));
    }

    #[test]
    fn helper_parsing() {
        assert_eq!(offset_to_minutes("+05:30"), Some(330));
        assert_eq!(offset_to_minutes("-08:00"), Some(-480));
        assert_eq!(offset_to_minutes("bogus"), None);
        assert_eq!(exif_datetime_to_iso("bad"), None);
        assert_eq!(exif_datetime_to_iso("é:::::::::::::::::"), None);
    }
}
