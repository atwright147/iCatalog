use super::folders::Folderizer;
use super::is_initialised;
use super::scan::{ingest, note_skip, parse_recursive, skip_rows, walk, Ingested, SkipReason};
use crate::{Command, Context, EngineError, Outcome, ParamSpec, Params, Result};
use std::path::Path;

pub(crate) struct Import;

/// Source kinds accepted by `import --kind` (D21).
const SOURCE_KINDS: &[&str] = &["local", "optical", "removable", "network"];

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
            ParamSpec {
                name: "kind",
                required: false,
                positional: false,
                doc: "source kind: local (default), optical, removable or network",
            },
        ]
    }
    fn enabled(&self, ctx: &Context) -> bool {
        is_initialised(ctx)
    }
    fn run(&self, ctx: &mut Context, params: &Params) -> Result<Outcome> {
        let root = params.require("path")?;
        let recursive = parse_recursive(params.get("recursive"));
        let kind = params.get("kind").unwrap_or("local");
        if !SOURCE_KINDS.contains(&kind) {
            return Err(EngineError::BadParam(format!(
                "unknown source kind '{kind}' (expected one of {})",
                SOURCE_KINDS.join(", ")
            )));
        }
        if !std::fs::metadata(root)?.is_dir() {
            return Err(EngineError::BadParam(format!("not a directory: {root}")));
        }
        // Scan an absolute root so catalogued paths, folder chains and the
        // source's `last_mount_path` all agree without resolving symlinks.
        let root_abs = std::path::absolute(root).unwrap_or_else(|_| Path::new(root).to_path_buf());
        // The import root is the source; its directory tree becomes the folder
        // tree, and each asset points at the folder for its own directory.
        let mut folderizer = Folderizer::for_import(ctx, &root_abs, kind)?;
        let (mut scanned, mut imported, mut duplicates, mut skipped, mut errors) =
            (0u64, 0u64, 0u64, 0u64, 0u64);
        let mut skips: Vec<(SkipReason, u64)> = Vec::new();
        let w = walk(&root_abs, recursive);
        errors += w.errors;
        for f in &w.files {
            scanned += 1;
            let folder_id = folderizer.folder_for(ctx, f)?;
            match ingest(ctx, f, false, folder_id) {
                Ok(Ingested::Added { errors: e }) => {
                    imported += 1;
                    errors += e;
                }
                Ok(Ingested::Duplicate) => duplicates += 1,
                Ok(Ingested::Skipped(s)) => {
                    skipped += 1;
                    note_skip(&mut skips, s);
                }
                Ok(Ingested::Failed) => errors += 1,
                Ok(Ingested::Changed { .. } | Ingested::Unchanged) => {}
                Err(e) => return Err(e),
            }
        }
        let mut rows: Vec<Vec<String>> = [
            ("scanned", scanned),
            ("imported", imported),
            ("duplicates", duplicates),
            ("skipped", skipped),
            ("errors", errors),
        ]
        .iter()
        .map(|(k, v)| vec![(*k).to_string(), v.to_string()])
        .collect();
        rows.extend(skip_rows(&skips));
        Ok(Outcome::table(vec!["metric".into(), "value".into()], rows))
    }
}
