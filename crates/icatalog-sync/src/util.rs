use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{Result, SyncError};

/// A compact, colon-free UTC timestamp (`YYYYMMDDTHHMMSSZ`) for backup names.
pub fn utc_stamp() -> String {
    icatalog_engine::utc_now_iso8601().replace(['-', ':'], "")
}

/// The directory holding `path`, or `.` when it has no parent, so a temp file
/// and its target rename stay on one filesystem.
pub fn parent_dir(path: &Path) -> PathBuf {
    match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

/// A hidden temp path in `dir`, unique per call (process id + clock).
pub fn temp_path(dir: &Path, tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    dir.join(format!(
        ".{tag}.icatalog-sync-{}-{nanos}.tmp",
        std::process::id()
    ))
}

/// The timestamped backup path for `local`: `name.rejected-<stamp>.sqlite`,
/// disambiguated with a counter if that name is taken.
pub fn backup_path(local: &Path, stamp: &str) -> PathBuf {
    let dir = parent_dir(local);
    let stem = local
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("catalogue");
    let mut candidate = dir.join(format!("{stem}.rejected-{stamp}.sqlite"));
    let mut n = 1u32;
    while candidate.exists() {
        candidate = dir.join(format!("{stem}.rejected-{stamp}-{n}.sqlite"));
        n = n.saturating_add(1);
    }
    candidate
}

/// Copy `local` to a timestamped backup next to it. Returns `None` when there
/// is no local file to preserve.
pub fn backup_local(local: &Path) -> Result<Option<PathBuf>> {
    if !local.exists() {
        return Ok(None);
    }
    let path = backup_path(local, &utc_stamp());
    std::fs::copy(local, &path)?;
    Ok(Some(path))
}

/// Verify that `path` is a valid iCatalog catalogue with the expected id.
pub fn verify_catalogue(path: &Path, expect_id: &str) -> Result<()> {
    let catalog = icatalog_catalog::Catalog::open(path)?;
    let row = catalog
        .catalogue()?
        .ok_or_else(|| SyncError::Integrity("not an iCatalog catalogue".into()))?;
    if row.id != expect_id {
        return Err(SyncError::Integrity(format!(
            "catalogue id '{}' does not match '{}'",
            row.id, expect_id
        )));
    }
    let check: String = catalog
        .connection()
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .map_err(icatalog_catalog::CatalogError::from)?;
    if check != "ok" {
        return Err(SyncError::Integrity(format!(
            "sqlite integrity_check: {check}"
        )));
    }
    Ok(())
}
