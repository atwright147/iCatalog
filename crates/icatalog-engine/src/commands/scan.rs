//! Shared bounded directory walk and per-file ingest, used by `import` and `update`.
use crate::{Context, Result};
use icatalog_catalog::{Annotation, Asset, AssetMeta, MetadataRaw, Thumbnail as CatalogThumbnail};
use icatalog_media::XmpData;
use std::path::{Path, PathBuf};

pub(super) const MAX_DEPTH: usize = 64;
pub(super) const MAX_FILE: u64 = 512 * 1024 * 1024;
const EXTS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "webp", "bmp", "tif", "tiff", "dng", "cr2", "cr3", "nef", "arw",
    "pef", "raf",
];

/// RAW extensions take the embedded-preview path. Content sniffing alone cannot
/// distinguish a DNG from a plain TIFF (both are TIFF-based and may embed a JPEG
/// preview), so the extension decides and the content sniff only validates it.
const RAW_EXTS: &[&str] = &["dng", "cr2", "cr3", "nef", "arw", "pef", "raf"];

/// Parse the optional `recursive` param (default true).
pub(super) fn parse_recursive(v: Option<&str>) -> bool {
    v.map(|v| !matches!(v, "false" | "0" | "no"))
        .unwrap_or(true)
}

/// Regular files found by [`walk`] plus the number of directory/entry errors.
pub(super) struct Walked {
    pub files: Vec<PathBuf>,
    pub errors: u64,
}

/// Bounded walk: depth <= `MAX_DEPTH`, symlinks never followed.
pub(super) fn walk(root: &Path, recursive: bool) -> Walked {
    let mut out = Walked {
        files: Vec::new(),
        errors: 0,
    };
    let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            out.errors += 1;
            continue;
        };
        for entry in rd {
            let Ok(entry) = entry else {
                out.errors += 1;
                continue;
            };
            let Ok(ft) = entry.file_type() else {
                out.errors += 1;
                continue;
            };
            if ft.is_symlink() {
                continue;
            }
            if ft.is_dir() {
                if recursive && depth < MAX_DEPTH {
                    stack.push((entry.path(), depth + 1));
                }
            } else if ft.is_file() {
                out.files.push(entry.path());
            }
        }
    }
    out
}

pub(super) fn has_image_ext(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Whether the extension marks a RAW container (see [`RAW_EXTS`]).
pub(super) fn has_raw_ext(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| RAW_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Why a candidate file was skipped rather than imported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SkipReason {
    /// Extension is not on the raster whitelist (`EXTS`).
    UnsupportedExt,
    /// Larger than `MAX_FILE`.
    TooLarge,
    /// Extension matched but the bytes are not a decodable image.
    NotImage,
    /// A RAW container with no usable embedded JPEG preview.
    RawNoPreview,
}

impl SkipReason {
    pub(super) fn label(self) -> &'static str {
        match self {
            SkipReason::UnsupportedExt => "unsupported file type",
            SkipReason::TooLarge => "larger than 512 MiB",
            SkipReason::NotImage => "not a recognised image",
            SkipReason::RawNoPreview => "RAW file with no usable preview",
        }
    }
}

/// Track per-reason skip counts.
pub(super) fn note_skip(skips: &mut Vec<(SkipReason, u64)>, s: SkipReason) {
    match skips.iter_mut().find(|(k, _)| *k == s) {
        Some((_, c)) => *c += 1,
        None => skips.push((s, 1)),
    }
}

/// Table rows ("skipped: <label>", count) for every reason observed.
pub(super) fn skip_rows(skips: &[(SkipReason, u64)]) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    for (s, c) in skips {
        if *c > 0 {
            rows.push(vec![format!("skipped: {}", s.label()), c.to_string()]);
        }
    }
    rows
}

/// What happened to one file.
pub(super) enum Ingested {
    Added { errors: u64 },
    Changed { errors: u64 },
    Unchanged,
    Duplicate,
    Skipped(SkipReason),
    Failed,
}

fn mtime_secs(md: &std::fs::Metadata) -> i64 {
    md.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// Write EXIF metadata and the grid thumbnail; returns the number of soft errors.
fn write_derived(
    ctx: &Context,
    asset_id: i64,
    image_bytes: &[u8],
    exif_bytes: &[u8],
    source: &str,
    file_size: i64,
    mtime: i64,
) -> Result<u64> {
    let mut errors = 0;
    let d = match icatalog_media::read_exif(std::io::Cursor::new(exif_bytes)) {
        Ok(d) => d,
        Err(_) => {
            errors += 1;
            Default::default()
        }
    };
    let orientation = d.orientation;
    // A changed file can lose its EXIF. Clear stale metadata so the display
    // fallback cannot rotate the new thumbnail using an old orientation.
    if d != Default::default() || ctx.catalog().asset_meta(asset_id)?.is_some() {
        ctx.catalog().upsert_asset_meta(&AssetMeta {
            asset_id,
            captured_local: d.captured_local,
            tz_offset_min: d.tz_offset_min,
            make: d.make,
            model: d.model,
            lens: d.lens,
            focal_length_mm: d.focal_length_mm,
            f_number: d.f_number,
            exposure_s: d.exposure_s,
            iso: d.iso,
            gps_lat: d.gps_lat,
            gps_lon: d.gps_lon,
            orientation: d.orientation,
            duration_ms: None,
        })?;
    }
    match icatalog_media::thumbnail_jpeg_with_orientation(
        image_bytes,
        icatalog_media::GRID_LONG_EDGE,
        orientation,
    ) {
        Ok(t) => ctx.catalog().insert_thumbnail(&CatalogThumbnail {
            asset_id,
            size_class: i64::from(icatalog_media::GRID_LONG_EDGE),
            source: source.into(),
            format: t.format,
            width: t.width,
            height: t.height,
            orientation_applied: t.orientation_applied,
            bytes: t.bytes,
            derived_from: Some(format!("{file_size}:{mtime}")),
            generator_version: icatalog_media::GENERATOR_VERSION,
        })?,
        Err(_) => errors += 1,
    }
    Ok(errors)
}

/// Locate and store the XMP packet verbatim (idempotent); returns the parsed data.
fn capture_xmp(
    ctx: &Context,
    asset_id: i64,
    primary: &[u8],
    fallback: Option<&[u8]>,
) -> Result<Option<XmpData>> {
    let Some(packet) = icatalog_media::xmp_packet(primary)
        .or_else(|| fallback.and_then(icatalog_media::xmp_packet))
    else {
        return Ok(None);
    };
    ctx.catalog().insert_metadata_raw(&MetadataRaw {
        asset_id,
        scheme: "xmp_packet".into(),
        key: "raw".into(),
        seq: 0,
        value: packet.as_bytes().to_vec(),
    })?;
    Ok(Some(icatalog_media::parse_xmp(&packet)))
}

/// Map an `xmp:Label` colour/name string to the numeric label 0-7.
/// Return None if there is no plausible mapping.
fn map_xmp_label(s: &str) -> Option<i64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    let t = t.to_ascii_lowercase();
    match t.as_str() {
        "red" => Some(1),
        "orange" => Some(2),
        "yellow" => Some(3),
        "green" => Some(4),
        "blue" => Some(5),
        "purple" => Some(6),
        "pink" => Some(7),
        "magenta" => Some(7),
        "violet" => Some(6),
        "indigo" => Some(6),
        "cyan" => Some(5),
        "teal" => Some(4),
        "lime" => Some(3),
        "brown" => Some(2),
        "black" => Some(1),
        "white" => Some(0),
        "gray" | "grey" => Some(0),
        _ => None,
    }
}

/// Seed rating/caption/keywords from XMP on first ingest only.
/// `xmp:Label` (colour name) is mapped to 0-7 when possible and only sets the
/// annotation label if the asset has no user value (label == 0).
fn seed_xmp(ctx: &Context, asset_id: i64, data: &XmpData) -> Result<()> {
    let mut ann = ctx.catalog().annotation(asset_id)?.unwrap_or(Annotation {
        asset_id,
        rating: 0,
        label: 0,
        caption: None,
        headline: None,
        creator: None,
        copyright: None,
        updated_utc: None,
    });
    let mut changed = false;
    if let Some(n) = data.rating {
        if (1..=5).contains(&n) && ann.rating == 0 {
            ann.rating = i64::from(n);
            changed = true;
        }
    }
    if let Some(lbl) = &data.label {
        if ann.label == 0 {
            if let Some(m) = map_xmp_label(lbl) {
                if (0..=7).contains(&m) {
                    ann.label = m;
                    changed = true;
                }
            }
        }
    }
    if let Some(d) = &data.description {
        if ann.caption.as_deref().unwrap_or("").is_empty() && !d.is_empty() {
            ann.caption = Some(d.clone());
            changed = true;
        }
    }
    if changed {
        ann.updated_utc = Some(super::edit::utc_iso_now());
        ctx.catalog().set_annotation(&ann)?;
    }
    for name in &data.keywords {
        if name.is_empty() {
            continue;
        }
        let kid = ctx.catalog().ensure_keyword(name, None)?;
        ctx.catalog().add_asset_keyword(asset_id, kid)?;
    }
    Ok(())
}

/// True if the RAW container itself yields EXIF (else the preview is tried).
fn raw_has_exif(bytes: &[u8]) -> bool {
    matches!(
        icatalog_media::read_exif(std::io::Cursor::new(bytes)),
        Ok(d) if d != Default::default()
    )
}

/// Ingest one file. With `refresh`, a file already catalogued at this path is
/// compared and updated in place (uid stays stable, `missing` is cleared);
/// without it, content already known by uid is a duplicate.
pub(super) fn ingest(
    ctx: &Context,
    path: &Path,
    refresh: bool,
    folder_id: Option<i64>,
) -> Result<Ingested> {
    if !has_image_ext(path) {
        return Ok(Ingested::Skipped(SkipReason::UnsupportedExt));
    }
    let Ok(md) = std::fs::metadata(path) else {
        return Ok(Ingested::Failed);
    };
    if md.len() > MAX_FILE {
        return Ok(Ingested::Skipped(SkipReason::TooLarge));
    }
    let file_size = i64::try_from(md.len()).unwrap_or(i64::MAX);
    let mtime = mtime_secs(&md);
    let path_str = path.to_string_lossy().into_owned();
    let mut existing = if refresh {
        ctx.catalog().asset_by_path(&path_str)?
    } else {
        None
    };
    let mut folder_changed = false;
    if let Some(a) = &mut existing {
        if let Some(fid) = folder_id {
            if a.folder_id != Some(fid) {
                a.folder_id = Some(fid);
                folder_changed = true;
            }
        }
        if a.file_size == Some(file_size) && a.mtime == Some(mtime) {
            if folder_changed {
                ctx.catalog().update_asset(a)?;
            }
            if a.missing {
                ctx.catalog().set_asset_missing(a.id, false)?;
            }
            return Ok(Ingested::Unchanged);
        }
    }
    let Ok(bytes) = std::fs::read(path) else {
        return Ok(Ingested::Failed);
    };
    let preview: Option<Vec<u8>> = if has_raw_ext(path) {
        match icatalog_media::raw_preview(&bytes) {
            Ok(p) if !p.is_empty() => Some(p),
            _ => return Ok(Ingested::Skipped(SkipReason::RawNoPreview)),
        }
    } else {
        None
    };
    let image_bytes: &[u8] = preview.as_deref().unwrap_or(&bytes);
    let source = if preview.is_some() {
        "raw_preview"
    } else {
        "rendered"
    };
    let Ok(info) = icatalog_media::probe(image_bytes) else {
        return Ok(if preview.is_some() {
            Ingested::Skipped(SkipReason::RawNoPreview)
        } else {
            Ingested::Skipped(SkipReason::NotImage)
        });
    };
    let exif_bytes: &[u8] = match &preview {
        Some(p) if !raw_has_exif(&bytes) => p,
        _ => &bytes,
    };
    let (format, width, height) = if preview.is_some() {
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_else(|| info.format.clone());
        // Never the preview's dimensions: true size or unknown (NULL).
        let dims = icatalog_media::raw_dimensions(&bytes);
        (
            ext,
            dims.map(|(w, _)| i64::from(w)),
            dims.map(|(_, h)| i64::from(h)),
        )
    } else {
        (info.format.clone(), Some(info.width), Some(info.height))
    };
    let hash = icatalog_media::content_hash(&bytes);
    if let Some(mut a) = existing {
        let same = a.content_hash.as_deref() == Some(hash.as_str());
        a.file_size = Some(file_size);
        a.mtime = Some(mtime);
        a.missing = false;
        if same {
            ctx.catalog().update_asset(&a)?;
            return Ok(Ingested::Unchanged);
        }
        a.content_hash = Some(hash);
        a.format = Some(format);
        a.width = width;
        a.height = height;
        ctx.catalog().update_asset(&a)?;
        let errors = write_derived(ctx, a.id, image_bytes, exif_bytes, source, file_size, mtime)?;
        capture_xmp(ctx, a.id, &bytes, preview.as_deref())?;
        return Ok(Ingested::Changed { errors });
    }
    if ctx.catalog().asset_by_uid(&hash)?.is_some() {
        return Ok(Ingested::Duplicate);
    }
    let a = Asset {
        id: 0,
        uid: hash.clone(),
        folder_id,
        path: path_str,
        path_raw: None,
        name: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        name_raw: None,
        format: Some(format),
        file_size: Some(file_size),
        width,
        height,
        color_space: None,
        mtime: Some(mtime),
        content_hash: Some(hash),
        missing: false,
    };
    let asset_id = ctx.catalog().insert_asset(&a)?;
    let errors = write_derived(
        ctx,
        asset_id,
        image_bytes,
        exif_bytes,
        source,
        file_size,
        mtime,
    )?;
    if let Some(data) = capture_xmp(ctx, asset_id, &bytes, preview.as_deref())? {
        seed_xmp(ctx, asset_id, &data)?;
    }
    Ok(Ingested::Added { errors })
}
