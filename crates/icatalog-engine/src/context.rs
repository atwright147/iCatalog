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
}
