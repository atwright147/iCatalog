use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{Result, SyncError};

/// Reserved temporary file, removed on every return path (including unwinding).
pub struct TempPath(std::path::PathBuf);
impl TempPath {
    pub fn reserve(local: &Path, tag: &str) -> Result<Self> {
        for _ in 0..100 {
            let path = temp_path(&parent_dir(local), tag);
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(_) => return Ok(Self(path)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e.into()),
            }
        }
        Err(SyncError::Io(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "cannot reserve a sync temporary file",
        )))
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for TempPath {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

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

pub fn sync_parent(path: &Path) -> Result<()> {
    #[cfg(unix)]
    std::fs::File::open(parent_dir(path))?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// SHA-256 a regular file in bounded chunks for interrupted-swap identity checks.
pub fn sha256_file(path: &Path) -> Result<String> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() {
        return Err(SyncError::Integrity(
            "cannot hash a non-regular sync file".into(),
        ));
    }
    if metadata.len() > crate::protocol::MAX_SNAPSHOT_BYTES {
        return Err(SyncError::Integrity(
            "sync file exceeds the snapshot limit".into(),
        ));
    }
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let count = std::io::Read::read(&mut file, &mut buffer)?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(count as u64)
            .ok_or_else(|| SyncError::Integrity("sync file length overflow".into()))?;
        if total > crate::protocol::MAX_SNAPSHOT_BYTES {
            return Err(SyncError::Integrity(
                "sync file exceeds the snapshot limit".into(),
            ));
        }
        hasher.update(
            buffer
                .get(..count)
                .ok_or_else(|| SyncError::Integrity("invalid file read length".into()))?,
        );
    }
    let digest = hasher.finalize();
    let mut text = String::with_capacity(64);
    for byte in digest {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let high = HEX
            .get(usize::from(byte >> 4))
            .copied()
            .ok_or_else(|| SyncError::Integrity("invalid digest value".into()))?;
        let low = HEX
            .get(usize::from(byte & 0x0f))
            .copied()
            .ok_or_else(|| SyncError::Integrity("invalid digest value".into()))?;
        text.push(char::from(high));
        text.push(char::from(low));
    }
    Ok(text)
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

/// Reserve a distinct backup without overwriting any existing file or symlink.
/// The caller holds the local SQLite write reservation in DELETE journal mode.
pub fn backup_local(local: &Path) -> Result<Option<PathBuf>> {
    let mut source = match std::fs::File::open(local) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let stem = local
        .file_stem()
        .unwrap_or_else(|| std::ffi::OsStr::new("catalogue"));
    let extension = local
        .extension()
        .unwrap_or_else(|| std::ffi::OsStr::new("icatalog"));
    let stamp = utc_stamp();
    for n in 0..10000u32 {
        let mut name = stem.to_os_string();
        name.push(if n == 0 {
            format!(".rejected-{stamp}")
        } else {
            format!(".rejected-{stamp}-{n}")
        });
        name.push(".");
        name.push(extension);
        let path = parent_dir(local).join(name);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                let result = std::io::copy(&mut source, &mut file).and_then(|_| file.sync_all());
                if let Err(e) = result {
                    let _ = std::fs::remove_file(&path);
                    return Err(e.into());
                }
                sync_parent(&path)?;
                return Ok(Some(path));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }
    }
    Err(SyncError::Protocol(
        "too many existing backups; choose another catalogue directory".into(),
    ))
}

/// Verify that `path` is a valid iCatalog catalogue with the expected id.
pub fn verify_catalogue(path: &Path, expect_id: &str) -> Result<()> {
    let catalog = icatalog_catalog::Catalog::open_existing(path)?;
    let row = catalog.catalogue()?.ok_or_else(|| {
        SyncError::Integrity(format!("not an {} catalogue", icatalog_catalog::APP_NAME))
    })?;
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
    let mut foreign_keys = catalog
        .connection()
        .prepare("PRAGMA foreign_key_check")
        .map_err(icatalog_catalog::CatalogError::from)?;
    if foreign_keys
        .exists([])
        .map_err(icatalog_catalog::CatalogError::from)?
    {
        return Err(SyncError::Integrity(
            "snapshot has broken foreign-key relationships".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]
    use super::*;
    #[test]
    fn backups_are_distinct_and_temporary_files_are_removed() {
        let dir = std::env::temp_dir().join(format!("icat-backup-v2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let local = dir.join("local.sqlite");
        std::fs::write(&local, b"original").unwrap();
        let a = backup_local(&local).unwrap().unwrap();
        let b = backup_local(&local).unwrap().unwrap();
        assert_ne!(a, b);
        assert_eq!(std::fs::read(a).unwrap(), b"original");
        let temp = TempPath::reserve(&local, "check").unwrap();
        let path = temp.path().to_path_buf();
        assert!(path.exists());
        drop(temp);
        assert!(!path.exists());
        let preferred = dir.join("library.icatalog");
        std::fs::write(&preferred, b"preferred").unwrap();
        let copy = backup_local(&preferred).unwrap().unwrap();
        assert_eq!(
            copy.extension().and_then(|ext| ext.to_str()),
            Some("icatalog")
        );
        assert_eq!(std::fs::read(copy).unwrap(), b"preferred");
    }
}
