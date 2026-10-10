use super::is_initialised;
use crate::{Command, Context, EngineError, Outcome, ParamSpec, Params, Result};
use icatalog_catalog::{Asset, AssetMeta, Thumbnail as CatalogThumbnail};
use std::path::PathBuf;

pub(crate) struct Import;

const MAX_DEPTH: usize = 64;
const MAX_FILE: u64 = 512 * 1024 * 1024;
const EXTS: &[&str] = &["jpg", "jpeg", "png", "gif", "webp", "bmp", "tif", "tiff"];

impl Command for Import {
    fn id(&self) -> &'static str {
        "import"
    }
    fn label(&self) -> &'static str {
        "Import images from a directory"
    }
    fn params(&self) -> &'static [ParamSpec] {
        &[
            ParamSpec {
                name: "path",
                required: true,
                positional: false,
                doc: "directory to scan",
            },
            ParamSpec {
                name: "recursive",
                required: false,
                positional: false,
                doc: "recurse into subdirectories (default true)",
            },
        ]
    }
    fn enabled(&self, ctx: &Context) -> bool {
        is_initialised(ctx)
    }
    fn run(&self, ctx: &mut Context, params: &Params) -> Result<Outcome> {
        let root = params.require("path")?;
        let recursive = params
            .get("recursive")
            .map(|v| !matches!(v, "false" | "0" | "no"))
            .unwrap_or(true);
        if !std::fs::metadata(root)?.is_dir() {
            return Err(EngineError::BadParam(format!("not a directory: {root}")));
        }
        let (mut scanned, mut imported, mut duplicates, mut skipped, mut errors) =
            (0u64, 0u64, 0u64, 0u64, 0u64);
        let mut stack: Vec<(PathBuf, usize)> = vec![(PathBuf::from(root), 0)];
        while let Some((dir, depth)) = stack.pop() {
            let rd = match std::fs::read_dir(&dir) {
                Ok(r) => r,
                Err(_) => {
                    errors += 1;
                    continue;
                }
            };
            for entry in rd {
                let Ok(entry) = entry else {
                    errors += 1;
                    continue;
                };
                let path = entry.path();
                let Ok(ft) = entry.file_type() else {
                    errors += 1;
                    continue;
                };
                if ft.is_symlink() {
                    continue;
                }
                if ft.is_dir() {
                    if recursive && depth < MAX_DEPTH {
                        stack.push((path, depth + 1));
                    }
                    continue;
                }
                if !ft.is_file() {
                    continue;
                }
                scanned += 1;
                let ext_ok = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| EXTS.contains(&e.to_ascii_lowercase().as_str()))
                    .unwrap_or(false);
                if !ext_ok {
                    skipped += 1;
                    continue;
                }
                let Ok(md) = entry.metadata() else {
                    errors += 1;
                    continue;
                };
                if md.len() > MAX_FILE {
                    skipped += 1;
                    continue;
                }
                let Ok(bytes) = std::fs::read(&path) else {
                    errors += 1;
                    continue;
                };
                let Ok(info) = icatalog_media::probe(&bytes) else {
                    skipped += 1;
                    continue;
                };
                let hash = icatalog_media::content_hash(&bytes);
                let uid = hash.clone();
                if ctx.catalog().asset_by_uid(&uid)?.is_some() {
                    duplicates += 1;
                    continue;
                }
                let file_size = i64::try_from(md.len()).unwrap_or(i64::MAX);
                let mtime = md
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
                    .unwrap_or(0);
                let a = Asset {
                    id: 0,
                    uid,
                    folder_id: None,
                    path: path.to_string_lossy().into_owned(),
                    path_raw: None,
                    name: entry.file_name().to_string_lossy().into_owned(),
                    name_raw: None,
                    format: Some(info.format),
                    file_size: Some(file_size),
                    width: Some(info.width),
                    height: Some(info.height),
                    color_space: None,
                    mtime: Some(mtime),
                    content_hash: Some(hash),
                };
                let asset_id = ctx.catalog().insert_asset(&a)?;
                match icatalog_media::read_exif(std::io::Cursor::new(&bytes)) {
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
                match icatalog_media::thumbnail_jpeg(&bytes, icatalog_media::GRID_LONG_EDGE) {
                    Ok(t) => {
                        ctx.catalog().insert_thumbnail(&CatalogThumbnail {
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
                        })?;
                    }
                    Err(_) => errors += 1,
                }
                imported += 1;
            }
        }
        let rows = [
            ("scanned", scanned),
            ("imported", imported),
            ("duplicates", duplicates),
            ("skipped", skipped),
            ("errors", errors),
        ]
        .iter()
        .map(|(k, v)| vec![(*k).to_string(), v.to_string()])
        .collect();
        Ok(Outcome::table(vec!["metric".into(), "value".into()], rows))
    }
}
