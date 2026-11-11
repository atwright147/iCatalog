//! Preview storage shared by engine commands and pull-only sync.
use crate::{EngineError, Result};
use icatalog_catalog::{valid_preview_hash, Catalog, MAX_PREVIEW_BYTES};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub fn hash(bytes: &[u8]) -> String {
    icatalog_media::content_hash(bytes)
        .strip_prefix("sha256:")
        .unwrap_or("")
        .to_owned()
}
fn parent(path: &Path) -> &Path {
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}
fn store_dir(path: &Path) -> Result<PathBuf> {
    let dir = parent(path).join("previews");
    match std::fs::symlink_metadata(&dir) {
        Ok(m) if !m.is_dir() || m.file_type().is_symlink() => {
            return Err(EngineError::Unsupported(
                "preview directory must be a real directory".into(),
            ))
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir(&dir)?;
        }
        Err(e) => return Err(e.into()),
    }
    Ok(dir)
}
fn checked_hash(hash: &str) -> Result<()> {
    if valid_preview_hash(hash) {
        Ok(())
    } else {
        Err(EngineError::BadParam(
            "preview hash must be 64 lowercase hexadecimal characters".into(),
        ))
    }
}
pub fn read(c: &Catalog, path: &Path, hash: &str) -> Result<Option<Vec<u8>>> {
    checked_hash(hash)?;
    if !c.preview_is_referenced(hash)? {
        return Ok(None);
    }
    let embedded = match c.preview_bytes(hash) {
        Ok(bytes) => bytes,
        Err(icatalog_catalog::CatalogError::CorruptPreview) => {
            return Err(EngineError::CorruptPreview)
        }
        Err(e) => return Err(e.into()),
    };
    let bytes = if let Some(bytes) = embedded {
        bytes
    } else {
        let dir = parent(path).join("previews");
        if let Ok(m) = std::fs::symlink_metadata(&dir) {
            if m.file_type().is_symlink() || !m.is_dir() {
                return Err(EngineError::Unsupported("invalid preview directory".into()));
            }
        }
        let file = match std::fs::File::open(dir.join(format!("{hash}.jpg"))) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let mut bytes = Vec::new();
        file.take((MAX_PREVIEW_BYTES as u64) + 1)
            .read_to_end(&mut bytes)?;
        bytes
    };
    if bytes.len() > MAX_PREVIEW_BYTES || self::hash(&bytes) != hash {
        return Err(EngineError::CorruptPreview);
    }
    Ok(Some(bytes))
}
/// Treat only invalid cached bytes as absent, leaving I/O and catalogue failures visible.
pub fn read_recoverable(c: &Catalog, path: &Path, hash: &str) -> Result<Option<Vec<u8>>> {
    match read(c, path, hash) {
        Err(EngineError::CorruptPreview) => Ok(None),
        other => other,
    }
}
fn write_external(path: &Path, hash: &str, bytes: &[u8]) -> Result<()> {
    let dir = store_dir(path)?;
    let mut slot = None;
    for n in 0..100u32 {
        let p = dir.join(format!(".{hash}-{}-{n}.tmp", std::process::id()));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&p)
        {
            Ok(f) => {
                slot = Some((p, f));
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }
    }
    let (temp, mut file) = slot
        .ok_or_else(|| EngineError::Internal("cannot reserve a preview temporary file".into()))?;
    let result = (|| -> Result<()> {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, dir.join(format!("{hash}.jpg")))?;
        #[cfg(unix)]
        std::fs::File::open(&dir)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temp);
    }
    result
}
/// Cache only a referenced, hash-verified preview; never reads original media.
pub fn cache(c: &Catalog, path: &Path, hash: &str, bytes: &[u8]) -> Result<()> {
    checked_hash(hash)?;
    if bytes.len() > MAX_PREVIEW_BYTES || self::hash(bytes) != hash {
        return Err(EngineError::BadParam(
            "preview failed its size or SHA-256 check".into(),
        ));
    }
    if !c.preview_is_referenced(hash)? {
        return Err(EngineError::BadParam(
            "preview is not referenced by this catalogue".into(),
        ));
    }
    store(c, path, hash, bytes, &c.preview_mode()?)
}
pub(crate) fn store(c: &Catalog, path: &Path, hash: &str, bytes: &[u8], mode: &str) -> Result<()> {
    checked_hash(hash)?;
    if bytes.len() > MAX_PREVIEW_BYTES || self::hash(bytes) != hash {
        return Err(EngineError::BadParam("invalid preview bytes".into()));
    }
    if mode == "external" {
        write_external(path, hash, bytes)?;
        c.store_preview_blob(hash, None)?;
    } else {
        c.store_preview_blob(hash, Some(bytes))?;
    }
    Ok(())
}
