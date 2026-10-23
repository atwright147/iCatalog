//! Source-rooted folder tree helpers shared by `import` and `update` (D21).
//!
//! A source is the root the user imported from (a directory, optical disc,
//! external drive or network share). Its folders mirror the directories under
//! that root; each asset points at the folder for its own directory.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use icatalog_catalog::Source;

use crate::{Context, Result};

/// Creates/reuses the folder chain for files under one source root.
pub(super) struct Folderizer {
    source_id: i64,
    root: PathBuf,
    /// Relative POSIX path -> folder id ("" is the source root folder).
    cache: HashMap<String, i64>,
}

impl Folderizer {
    /// Open the source at `root` (creating or refreshing it) and its root folder.
    pub(super) fn for_import(ctx: &Context, root: &Path, kind: &str) -> Result<Self> {
        let root = std::path::absolute(root).unwrap_or_else(|_| root.to_path_buf());
        let identity = root.to_string_lossy().into_owned();
        let label = root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| identity.clone());
        let source_id = ctx.catalog().ensure_source(
            kind,
            &label,
            &identity,
            &identity,
            &crate::time::utc_now_iso8601(),
        )?;
        Self::attach(ctx, source_id, &root, &label)
    }

    /// Attach to an existing source, but only when its root is currently
    /// present. An absent root means an offline disc or share: never create
    /// folders for it (and `update` treats its assets as offline, not missing).
    pub(super) fn for_source(ctx: &Context, source: &Source) -> Result<Option<Self>> {
        let Some(mount) = source.last_mount_path.as_deref() else {
            return Ok(None);
        };
        let root = PathBuf::from(mount);
        if !root.is_dir() {
            return Ok(None);
        }
        Ok(Some(Self::attach(ctx, source.id, &root, &source.label)?))
    }

    fn attach(ctx: &Context, source_id: i64, root: &Path, label: &str) -> Result<Self> {
        let root_id = match ctx.catalog().folder_by_source_path(source_id, "")? {
            Some(f) => f.id,
            None => ctx
                .catalog()
                .insert_folder_at(None, Some(source_id), label, Some(""))?,
        };
        let mut cache = HashMap::new();
        cache.insert(String::new(), root_id);
        Ok(Self {
            source_id,
            root: root.to_path_buf(),
            cache,
        })
    }

    /// Folder id for the directory containing `file`, creating the chain as
    /// needed. `None` when the file is not under this source's root.
    pub(super) fn folder_for(&mut self, ctx: &Context, file: &Path) -> Result<Option<i64>> {
        let Some(parent) = file.parent() else {
            return Ok(None);
        };
        let Ok(rel) = parent.strip_prefix(&self.root) else {
            return Ok(None);
        };
        let mut cur = String::new();
        let mut cur_id = self.cache.get("").copied();
        for comp in rel.components() {
            let std::path::Component::Normal(part) = comp else {
                continue;
            };
            let name = part.to_string_lossy();
            if name.is_empty() {
                continue;
            }
            if !cur.is_empty() {
                cur.push('/');
            }
            cur.push_str(&name);
            if let Some(id) = self.cache.get(&cur) {
                cur_id = Some(*id);
                continue;
            }
            let id = match ctx.catalog().folder_by_source_path(self.source_id, &cur)? {
                Some(f) => f.id,
                None => {
                    let Some(parent_id) = cur_id else {
                        return Ok(None);
                    };
                    ctx.catalog().insert_folder_at(
                        Some(parent_id),
                        Some(self.source_id),
                        &name,
                        Some(&cur),
                    )?
                }
            };
            self.cache.insert(cur.clone(), id);
            cur_id = Some(id);
        }
        Ok(cur_id)
    }
}

/// The source with the longest `last_mount_path` that contains `path`.
pub(super) fn source_for<'a>(sources: &'a [Source], path: &Path) -> Option<&'a Source> {
    sources
        .iter()
        .filter(|s| {
            s.last_mount_path
                .as_deref()
                .map(|root| path.starts_with(root))
                .unwrap_or(false)
        })
        .max_by_key(|s| s.last_mount_path.as_deref().map(str::len).unwrap_or(0))
}
