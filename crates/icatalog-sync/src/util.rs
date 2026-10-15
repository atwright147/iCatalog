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

/// A fast, non-cryptographic fingerprint of a byte slice: an FNV-1a-style hash
/// over 8-byte little-endian words, plus the length. It answers only "has the
/// local copy changed since the last sync?" (see `docs/sync.md` section 4); it
/// is never a security or integrity guarantee. Deterministic across runs,
/// processes and machines.
pub fn fingerprint(bytes: &[u8]) -> String {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    for chunk in bytes.chunks(8) {
        let mut word = [0u8; 8];
        // `chunks(8)` bounds the length at 8, so the ranges always match.
        word[..chunk.len()].copy_from_slice(chunk);
        hash ^= u64::from_le_bytes(word);
        hash = hash.wrapping_mul(PRIME);
    }
    format!("{hash:016x}-{}", bytes.len())
}

/// The local sync marker path for `local`: `<name>.sync-state` beside it. The
/// marker records the fingerprint of the last state that was in sync with the
/// server, so an unchanged copy can skip a push without bumping the generation.
pub fn state_path(local: &Path) -> PathBuf {
    let mut name = local
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_else(|| std::ffi::OsString::from("catalogue"));
    name.push(".sync-state");
    parent_dir(local).join(name)
}

/// The fingerprint recorded in the local sync marker, if any. A missing,
/// unreadable or malformed marker reads as "unknown", and the caller then
/// treats the copy as changed (which is always safe: it only causes a push).
pub fn read_state(local: &Path) -> Option<String> {
    let text = std::fs::read_to_string(state_path(local)).ok()?;
    for line in text.lines() {
        if let Some(value) = line.trim().strip_prefix("fingerprint=") {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// Record the fingerprint of the local copy after a successful push or pull.
/// Best effort: a missing or unreadable marker only means the next sync pushes
/// again, which is safe.
pub fn write_state(local: &Path, fingerprint: &str) {
    let _ = std::fs::write(state_path(local), format!("fingerprint={fingerprint}\n"));
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn fingerprint_is_deterministic_and_length_sensitive() {
        assert_eq!(fingerprint(b"hello"), fingerprint(b"hello"));
        assert_ne!(fingerprint(b"hello"), fingerprint(b"hellp"));
        assert_ne!(fingerprint(b"ab"), fingerprint(b"abc"));
        assert_ne!(fingerprint(b""), fingerprint(b"a"));
    }

    #[test]
    fn sync_marker_round_trips_and_tolerates_garbage() {
        let dir = std::env::temp_dir().join(format!("icat-marker-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let local = dir.join("local.sqlite");

        // No marker yet: unknown, so the caller treats the copy as changed.
        assert_eq!(read_state(&local), None);

        write_state(&local, "deadbeef-3");
        assert_eq!(read_state(&local).as_deref(), Some("deadbeef-3"));

        // Garbage is "unknown", never a panic.
        std::fs::write(state_path(&local), "not a marker\n").unwrap();
        assert_eq!(read_state(&local), None);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
