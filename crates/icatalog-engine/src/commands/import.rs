use super::is_initialised;
use super::scan::{ingest, note_skip, parse_recursive, skip_rows, walk, Ingested, SkipReason};
use crate::{Command, Context, EngineError, Outcome, ParamSpec, Params, Result};
use std::path::Path;

pub(crate) struct Import;

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
        let recursive = parse_recursive(params.get("recursive"));
        if !std::fs::metadata(root)?.is_dir() {
            return Err(EngineError::BadParam(format!("not a directory: {root}")));
        }
        let (mut scanned, mut imported, mut duplicates, mut skipped, mut errors) =
            (0u64, 0u64, 0u64, 0u64, 0u64);
        let mut skips: Vec<(SkipReason, u64)> = Vec::new();
        let w = walk(Path::new(root), recursive);
        errors += w.errors;
        for f in &w.files {
            scanned += 1;
            match ingest(ctx, f, false) {
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
