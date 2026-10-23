use super::folders::{source_for, Folderizer};
use super::is_initialised;
use super::require_catalogue;
use super::scan::{ingest, note_skip, parse_recursive, skip_rows, walk, Ingested, SkipReason};
use crate::{Command, Context, EngineError, Outcome, ParamSpec, Params, Result};
use icatalog_catalog::{Asset, AssetQuery};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

pub(crate) struct Update;

impl Command for Update {
    fn id(&self) -> &'static str {
        "update"
    }
    fn label(&self) -> &'static str {
        "Rescan folders for added, changed and missing files"
    }
    fn params(&self) -> &'static [ParamSpec] {
        &[
            ParamSpec {
                name: "path",
                required: false,
                positional: false,
                doc: "directory to rescan (default: every folder holding catalogued files, non-recursive)",
            },
            ParamSpec {
                name: "recursive",
                required: false,
                positional: false,
                doc: "recurse into subdirectories when --path is given (default true)",
            },
        ]
    }
    fn enabled(&self, ctx: &Context) -> bool {
        is_initialised(ctx)
    }
    fn run(&self, ctx: &mut Context, params: &Params) -> Result<Outcome> {
        require_catalogue(ctx)?;
        let recursive_param = params.get("recursive");
        let recursive = parse_recursive(recursive_param);
        if let Some(v) = recursive_param {
            if !matches!(v, "true" | "false" | "0" | "1" | "yes" | "no") {
                return Err(EngineError::BadParam(format!("bad recursive value: {v}")));
            }
        }
        let all = ctx.catalog().query_assets(&AssetQuery::default())?;
        let sources = ctx.catalog().all_sources()?;
        let folders = ctx.catalog().all_folders()?;
        let folder_source: HashMap<i64, i64> = folders
            .iter()
            .filter_map(|f| f.source_id.map(|s| (f.id, s)))
            .collect();
        // A source whose root is not currently mounted is offline (an optical
        // disc, an unplugged drive, an unavailable share). Never scan it and
        // never mark its assets missing: an offline source is not data loss.
        let offline: HashSet<i64> = sources
            .iter()
            .filter(|s| {
                s.last_mount_path
                    .as_deref()
                    .map(|p| !Path::new(p).is_dir())
                    .unwrap_or(true)
            })
            .map(|s| s.id)
            .collect();
        let offline_asset = |a: &Asset| {
            a.folder_id
                .and_then(|f| folder_source.get(&f).copied())
                .map(|s| offline.contains(&s))
                .unwrap_or(false)
        };
        let offline_count = u64::try_from(all.iter().filter(|a| offline_asset(a)).count())
            .unwrap_or(u64::MAX);
        // (root, recursive) scan scopes.
        let scopes: Vec<(PathBuf, bool)> = match params.get("path") {
            Some(p) => {
                let root =
                    std::path::absolute(p).unwrap_or_else(|_| PathBuf::from(p));
                if !std::fs::metadata(&root)?.is_dir() {
                    return Err(EngineError::BadParam(format!("not a directory: {p}")));
                }
                vec![(root, recursive)]
            }
            None => all
                .iter()
                .filter(|a| !offline_asset(a))
                .filter_map(|a| Path::new(&a.path).parent().map(Path::to_path_buf))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .map(|d| (d, false))
                .collect(),
        };
        let (mut scanned, mut added, mut changed, mut unchanged, mut skipped, mut errors) =
            (0u64, 0u64, 0u64, 0u64, 0u64, 0u64);
        let mut skips: Vec<(SkipReason, u64)> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        // One folder chain per source root, created on demand.
        let mut folderizers: HashMap<i64, Folderizer> = HashMap::new();
        for (root, rec) in &scopes {
            let w = walk(root, *rec);
            errors += w.errors;
            for f in &w.files {
                seen.insert(f.to_string_lossy().into_owned());
                scanned += 1;
                let mut folder_id = None;
                if let Some(src) = source_for(&sources, f) {
                    if let std::collections::hash_map::Entry::Vacant(e) = folderizers.entry(src.id) {
                        if let Some(fz) = Folderizer::for_source(ctx, src)? {
                            e.insert(fz);
                        }
                    }
                    if let Some(fz) = folderizers.get_mut(&src.id) {
                        folder_id = fz.folder_for(ctx, f)?;
                    }
                }
                match ingest(ctx, f, true, folder_id) {
                    Ok(Ingested::Added { errors: e }) => {
                        added += 1;
                        errors += e;
                    }
                    Ok(Ingested::Changed { errors: e }) => {
                        changed += 1;
                        errors += e;
                    }
                    Ok(Ingested::Unchanged | Ingested::Duplicate) => unchanged += 1,
                    Ok(Ingested::Skipped(s)) => {
                        skipped += 1;
                        note_skip(&mut skips, s);
                    }
                    Ok(Ingested::Failed) | Err(_) => errors += 1,
                }
            }
        }
        // Missing: catalogued files in scope that were not walked and are gone
        // from disk. Assets of an offline source are never considered missing.
        let mut missing = 0u64;
        for a in &all {
            if offline_asset(a) {
                continue;
            }
            let p = Path::new(&a.path);
            let in_scope = match params.get("path") {
                Some(_) => scopes.iter().any(|(root, rec)| {
                    p.starts_with(root) && (*rec || p.parent() == Some(root.as_path()))
                }),
                None => true,
            };
            if !in_scope || seen.contains(&a.path) {
                continue;
            }
            if std::fs::symlink_metadata(p).is_err() {
                missing += 1;
                if !a.missing {
                    ctx.catalog().set_asset_missing(a.id, true)?;
                }
            }
        }
        let mut rows: Vec<Vec<String>> = [
            ("scanned", scanned),
            ("added", added),
            ("changed", changed),
            ("unchanged", unchanged),
            ("skipped", skipped),
            ("missing", missing),
            ("offline", offline_count),
            ("errors", errors),
        ]
        .iter()
        .map(|(k, v)| vec![(*k).to_string(), v.to_string()])
        .collect();
        rows.extend(skip_rows(&skips));
        Ok(Outcome::table(vec!["metric".into(), "value".into()], rows))
    }
}
