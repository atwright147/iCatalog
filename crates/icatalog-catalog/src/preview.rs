//! Preview references and optional embedded bytes. Hash validation precedes paths.
use crate::{Catalog, CatalogError, Result};
use rusqlite::{params, OptionalExtension};

pub const MAX_PREVIEW_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    pub asset_id: i64,
    pub hash: String,
    pub width: i64,
    pub height: i64,
    pub source_hash: String,
}

pub fn valid_hash(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl Catalog {
    pub fn preview_mode(&self) -> Result<String> {
        Ok(self.conn.query_row(
            "SELECT mode FROM preview_setting WHERE singleton=1",
            [],
            |r| r.get(0),
        )?)
    }
    pub fn set_preview_mode(&self, mode: &str) -> Result<()> {
        if !matches!(mode, "embedded" | "external") {
            return Err(CatalogError::InvalidQuery(
                "preview mode must be embedded or external",
            ));
        }
        self.conn.execute(
            "UPDATE preview_setting SET mode=?1 WHERE singleton=1 AND mode!=?1",
            [mode],
        )?;
        Ok(())
    }
    pub fn preview(&self, asset_id: i64) -> Result<Option<Preview>> {
        Ok(self
            .conn
            .query_row(
                "SELECT asset_id,hash,width,height,source_hash FROM preview WHERE asset_id=?1",
                [asset_id],
                |r| {
                    Ok(Preview {
                        asset_id: r.get(0)?,
                        hash: r.get(1)?,
                        width: r.get(2)?,
                        height: r.get(3)?,
                        source_hash: r.get(4)?,
                    })
                },
            )
            .optional()?)
    }
    pub fn preview_hashes(&self) -> Result<Vec<String>> {
        let mut s = self
            .conn
            .prepare("SELECT DISTINCT hash FROM preview ORDER BY hash LIMIT 100001")?;
        let rows = s.query_map([], |r| r.get(0))?;
        let hashes = rows.collect::<std::result::Result<Vec<_>, _>>()?;
        if hashes.len() > crate::MAX_SYNC_ROWS {
            return Err(CatalogError::InvalidQuery(
                "too many previews to process at once",
            ));
        }
        Ok(hashes)
    }
    pub fn preview_is_referenced(&self, hash: &str) -> Result<bool> {
        Ok(self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM preview WHERE hash=?1)",
            [hash],
            |r| r.get(0),
        )?)
    }
    pub fn preview_bytes(&self, hash: &str) -> Result<Option<Vec<u8>>> {
        if !valid_hash(hash) {
            return Err(CatalogError::InvalidQuery("invalid preview hash"));
        }
        let length: Option<i64> = self
            .conn
            .query_row(
                "SELECT length(bytes) FROM preview_blob WHERE hash=?1",
                [hash],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        if length.is_some_and(|n| n < 0 || n > MAX_PREVIEW_BYTES as i64) {
            return Err(CatalogError::InvalidQuery("preview exceeds 16 MiB"));
        }
        Ok(self
            .conn
            .query_row(
                "SELECT bytes FROM preview_blob WHERE hash=?1",
                [hash],
                |r| r.get(0),
            )
            .optional()?
            .flatten())
    }
    pub fn store_preview_blob(&self, hash: &str, bytes: Option<&[u8]>) -> Result<()> {
        if !valid_hash(hash) || bytes.is_some_and(|b| b.len() > MAX_PREVIEW_BYTES) {
            return Err(CatalogError::InvalidQuery("invalid preview hash or size"));
        }
        self.conn.execute("INSERT INTO preview_blob(hash,bytes) VALUES(?1,?2) ON CONFLICT(hash) DO UPDATE SET bytes=excluded.bytes WHERE bytes IS NOT excluded.bytes",params![hash,bytes])?;
        Ok(())
    }
    pub fn store_preview(&self, p: &Preview) -> Result<()> {
        self.conn.execute("INSERT INTO preview(asset_id,hash,width,height,source_hash) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(asset_id) DO UPDATE SET hash=excluded.hash,width=excluded.width,height=excluded.height,source_hash=excluded.source_hash",params![p.asset_id,p.hash,p.width,p.height,p.source_hash])?;
        Ok(())
    }
}
