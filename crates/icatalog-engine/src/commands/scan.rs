//! Shared bounded directory walk and per-file ingest, used by `import` and `update`.
use crate::{Context, Result};
use icatalog_catalog::{Asset, AssetMeta, Thumbnail as CatalogThumbnail};
use std::path::{Path, PathBuf};

pub(super) const MAX_DEPTH: usize = 64;
pub(super) const MAX_FILE: u64 = 512 * 1024 * 1024;
const EXTS: &[&str] = &["jpg", "jpeg", "png", "gif", "webp", "bmp", "tif", "tiff"];

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

/// What happened to one file.
pub(super) enum Ingested {
    Added { errors: u64 },
    Changed { errors: u64 },
    Unchanged,
    Duplicate,
    Skipped,
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
    bytes: &[u8],
    file_size: i64,
    mtime: i64,
) -> Result<u64> {
    let mut errors = 0;
    match icatalog_media::read_exif(std::io::Cursor::new(bytes)) {
        Ok(d) => {
            if d != Default::default() {
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
        }
        Err(_) => errors += 1,
    }
    match icatalog_media::thumbnail_jpeg(bytes, icatalog_media::GRID_LONG_EDGE) {
        Ok(t) => ctx.catalog().insert_thumbnail(&CatalogThumbnail {
            asset_id,
            size_class: i64::from(icatalog_media::GRID_LONG_EDGE),
            source: "rendered".into(),
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

/// Ingest one file. With `refresh`, a file already catalogued at this path is
/// compared and updated in place (uid stays stable, `missing` is cleared);
/// without it, content already known by uid is a duplicate.
pub(super) fn ingest(ctx: &Context, path: &Path, refresh: bool) -> Result<Ingested> {
    if !has_image_ext(path) {
        return Ok(Ingested::Skipped);
    }
    let Ok(md) = std::fs::metadata(path) else {
        return Ok(Ingested::Failed);
    };
    if md.len() > MAX_FILE {
        return Ok(Ingested::Skipped);
    }
    let file_size = i64::try_from(md.len()).unwrap_or(i64::MAX);
    let mtime = mtime_secs(&md);
    let path_str = path.to_string_lossy().into_owned();
    let existing = if refresh {
        ctx.catalog().asset_by_path(&path_str)?
    } else {
        None
    };
    if let Some(a) = &existing {
        if a.file_size == Some(file_size) && a.mtime == Some(mtime) {
            if a.missing {
                ctx.catalog().set_asset_missing(a.id, false)?;
            }
            return Ok(Ingested::Unchanged);
        }
    }
    let Ok(bytes) = std::fs::read(path) else {
        return Ok(Ingested::Failed);
    };
    let Ok(info) = icatalog_media::probe(&bytes) else {
        return Ok(Ingested::Skipped);
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
        a.format = Some(info.format);
        a.width = Some(info.width);
        a.height = Some(info.height);
        ctx.catalog().update_asset(&a)?;
        let errors = write_derived(ctx, a.id, &bytes, file_size, mtime)?;
        return Ok(Ingested::Changed { errors });
    }
    if ctx.catalog().asset_by_uid(&hash)?.is_some() {
        return Ok(Ingested::Duplicate);
    }
    let a = Asset {
        id: 0,
        uid: hash.clone(),
        folder_id: None,
        path: path_str,
        path_raw: None,
        name: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        name_raw: None,
        format: Some(info.format),
        file_size: Some(file_size),
        width: Some(info.width),
        height: Some(info.height),
        color_space: None,
        mtime: Some(mtime),
        content_hash: Some(hash),
        missing: false,
    };
    let asset_id = ctx.catalog().insert_asset(&a)?;
    let errors = write_derived(ctx, asset_id, &bytes, file_size, mtime)?;
    Ok(Ingested::Added { errors })
}
