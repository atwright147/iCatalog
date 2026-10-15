use crate::error::Result;
use std::path::{Path, PathBuf};

/// An open catalogue session.
pub struct Context {
    path: PathBuf,
    catalog: icatalog_catalog::Catalog,
}

impl Context {
    /// Open (creating if absent) the catalogue at `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let catalog = icatalog_catalog::Catalog::open(&path)?;
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

    /// Replace the on-disk catalogue with `temp` (a verified catalogue file on
    /// the same filesystem), closing the connection so the rename is atomic,
    /// then reopen it. On failure the existing file is left in place and the
    /// connection is reopened. Used by the sync client's download-and-swap.
    pub fn replace_catalogue_file(&mut self, temp: &Path) -> Result<()> {
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
