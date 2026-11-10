use crate::error::{EngineError, Result};
use std::path::{Path, PathBuf};

/// An open catalogue session.
pub struct Context {
    path: PathBuf,
    catalog: icatalog_catalog::Catalog,
}

impl Context {
    /// Open a session backed by an in-memory catalogue, primarily for tests.
    pub fn open_in_memory() -> Result<Self> {
        Ok(Context {
            path: PathBuf::from(":memory:"),
            catalog: icatalog_catalog::Catalog::open_in_memory()?,
        })
    }

    /// Prepare a destination session without opening, creating or migrating
    /// its file. Used by commands that install a verified new catalogue.
    pub fn for_destination(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Context {
            path: path.as_ref().to_path_buf(),
            catalog: icatalog_catalog::Catalog::open_in_memory()?,
        })
    }

    /// Open (creating if absent) the catalogue at `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let catalog = icatalog_catalog::Catalog::open(&path)?;
        Ok(Context { path, catalog })
    }

    /// Open an existing on-disk catalogue without creating a missing file.
    pub fn open_existing(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if !path.is_file() {
            return Err(crate::error::EngineError::BadParam(
                "catalogue must be an existing file; check its path and mounts, or run init explicitly"
                    .into(),
            ));
        }
        // Removing SQLite's create flag also covers removal between the check
        // above and opening the connection.
        let catalog = icatalog_catalog::Catalog::open_existing(&path)?;
        Ok(Context { path, catalog })
    }

    /// Catalogue file path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Shared access to the catalogue.
    pub fn catalog(&self) -> &icatalog_catalog::Catalog {
        &self.catalog
    }

    /// Mutable access to the catalogue.
    pub fn catalog_mut(&mut self) -> &mut icatalog_catalog::Catalog {
        &mut self.catalog
    }

    /// Require a regular pathname before replacing this catalogue file.
    /// Replacing a symlink would detach the new replica from its original target.
    pub fn ensure_replaceable_path(&self) -> Result<()> {
        let metadata = std::fs::symlink_metadata(&self.path)?;
        let kind = metadata.file_type();
        if kind.is_symlink() {
            return Err(EngineError::Unsupported(
                "sync cannot replace a catalogue symlink; open its real file path".into(),
            ));
        }
        if !kind.is_file() {
            return Err(EngineError::Unsupported(
                "sync requires a regular catalogue file".into(),
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if metadata.nlink() > 1 {
                return Err(EngineError::Unsupported(
                    "sync cannot replace a catalogue with hard links; use a separate copy".into(),
                ));
            }
        }
        Ok(())
    }

    /// Replace the on-disk catalogue with `temp` (a verified catalogue file on
    /// the same filesystem), closing the connection so the rename is atomic,
    /// then reopen it. On failure the existing file is left in place and the
    /// connection is reopened. Used by the sync client's download-and-swap.
    pub fn replace_catalogue_file(&mut self, temp: &Path) -> Result<()> {
        self.ensure_replaceable_path()?;
        let placeholder = icatalog_catalog::Catalog::open_in_memory()?;
        let old = std::mem::replace(&mut self.catalog, placeholder);
        drop(old);
        if let Err(e) = std::fs::rename(temp, &self.path) {
            self.catalog = icatalog_catalog::Catalog::open(&self.path)?;
            return Err(e.into());
        }
        self.catalog = icatalog_catalog::Catalog::open(&self.path)?;
        Ok(())
    }
}
