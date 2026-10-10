//! SQLite catalogue schema and migrations for iCatalog.
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::fmt;
use std::path::Path;

use rusqlite::{params, Connection};

mod annotation;
mod folder;
mod metadata;
mod model;
mod query;

pub use model::{Annotation, AssetMeta, Collection, Folder, Keyword, MetadataRaw, Thumbnail};
pub use query::{AssetQuery, CatalogStats};

const MIGRATIONS: &[&str] = &[include_str!("v1.sql"), include_str!("v2.sql")];

/// Errors from opening, migrating or querying a catalogue.
#[derive(Debug)]
pub enum CatalogError {
    /// Underlying SQLite failure.
    Sqlite(rusqlite::Error),
    /// The catalogue was written by a newer iCatalog.
    NewerSchema { found: i64, supported: i64 },
    /// A recorded version is not a valid value.
    InvalidVersion(i64),
}

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CatalogError::Sqlite(e) => write!(f, "sqlite error: {e}"),
            CatalogError::NewerSchema { found, supported } => write!(
                f,
                "catalogue schema version {found} is newer than supported version {supported}; update iCatalog"
            ),
            CatalogError::InvalidVersion(v) => write!(f, "invalid schema version {v}"),
        }
    }
}

impl std::error::Error for CatalogError {}

impl From<rusqlite::Error> for CatalogError {
    fn from(e: rusqlite::Error) -> Self {
        CatalogError::Sqlite(e)
    }
}

pub type Result<T> = std::result::Result<T, CatalogError>;

/// A row of the `catalogue` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogueRow {
    pub id: String,
    pub generation: i64,
    pub created_utc: String,
}

/// A row of the `asset` table.
#[derive(Debug, Clone, PartialEq)]
pub struct Asset {
    pub id: i64,
    /// Stable id assigned by the caller at first import; never generated here.
    pub uid: String,
    pub folder_id: Option<i64>,
    /// Normalised POSIX path, for queries.
    pub path: String,
    /// Original legacy path bytes, for provenance.
    pub path_raw: Option<Vec<u8>>,
    pub name: String,
    pub name_raw: Option<Vec<u8>>,
    pub format: Option<String>,
    pub file_size: Option<i64>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub color_space: Option<i64>,
    /// File modification time, epoch seconds.
    pub mtime: Option<i64>,
    pub content_hash: Option<String>,
}

/// An open catalogue.
pub struct Catalog {
    conn: Connection,
}

pub(crate) fn asset_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Asset> {
    Ok(Asset {
        id: r.get(0)?,
        uid: r.get(1)?,
        folder_id: r.get(2)?,
        path: r.get(3)?,
        path_raw: r.get(4)?,
        name: r.get(5)?,
        name_raw: r.get(6)?,
        format: r.get(7)?,
        file_size: r.get(8)?,
        width: r.get(9)?,
        height: r.get(10)?,
        color_space: r.get(11)?,
        mtime: r.get(12)?,
        content_hash: r.get(13)?,
    })
}

fn latest_version() -> i64 {
    i64::try_from(MIGRATIONS.len()).unwrap_or(i64::MAX)
}

fn migrate(conn: &mut Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
           version INTEGER PRIMARY KEY,
           applied_utc TEXT NOT NULL
         );",
    )?;
    let current: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |r| r.get(0),
    )?;
    let latest = latest_version();
    if current < 0 {
        return Err(CatalogError::InvalidVersion(current));
    }
    if current > latest {
        return Err(CatalogError::NewerSchema {
            found: current,
            supported: latest,
        });
    }
    let skip = usize::try_from(current).map_err(|_| CatalogError::InvalidVersion(current))?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(skip) {
        let version = i64::try_from(i + 1).map_err(|_| CatalogError::InvalidVersion(-1))?;
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.execute(
            "INSERT INTO schema_migrations (version, applied_utc) VALUES (?1, strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
            params![version],
        )?;
        tx.commit()?;
    }
    Ok(())
}

impl Catalog {
    fn from_conn(mut conn: Connection) -> Result<Self> {
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        migrate(&mut conn)?;
        Ok(Catalog { conn })
    }

    /// Open (creating if absent) a catalogue file and apply pending migrations.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_conn(Connection::open(path)?)
    }

    /// Open a fresh in-memory catalogue.
    pub fn open_in_memory() -> Result<Self> {
        Self::from_conn(Connection::open_in_memory()?)
    }

    /// Highest applied schema version.
    pub fn schema_version(&self) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |r| r.get(0),
        )?)
    }

    /// Borrow the underlying connection.
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    pub fn insert_catalogue(&self, id: &str, created_utc: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO catalogue (id, generation, created_utc, schema_version) VALUES (?1, 0, ?2, ?3)",
            params![id, created_utc, self.schema_version()?],
        )?;
        Ok(())
    }

    pub fn catalogue(&self) -> Result<Option<CatalogueRow>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, generation, created_utc FROM catalogue LIMIT 1")?;
        let mut rows = stmt.query([])?;
        match rows.next()? {
            Some(r) => Ok(Some(CatalogueRow {
                id: r.get(0)?,
                generation: r.get(1)?,
                created_utc: r.get(2)?,
            })),
            None => Ok(None),
        }
    }

    /// Insert an asset (its `id` field is ignored) and return the new row id.
    pub fn insert_asset(&self, a: &Asset) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO asset (uid, folder_id, path, path_raw, name, name_raw, format, file_size, width, height, color_space, mtime, content_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                a.uid, a.folder_id, a.path, a.path_raw, a.name, a.name_raw, a.format,
                a.file_size, a.width, a.height, a.color_space, a.mtime, a.content_hash
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn asset(&self, id: i64) -> Result<Option<Asset>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, uid, folder_id, path, path_raw, name, name_raw, format, file_size, width, height, color_space, mtime, content_hash
             FROM asset WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id])?;
        match rows.next()? {
            Some(r) => Ok(Some(asset_from_row(r)?)),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn asset(uid: &str, path_raw: Option<Vec<u8>>, name_raw: Option<Vec<u8>>) -> Asset {
        Asset {
            id: 0,
            uid: uid.into(),
            folder_id: None,
            path: "a/b".into(),
            path_raw,
            name: "b".into(),
            name_raw,
            format: Some("JPEG".into()),
            file_size: Some(10),
            width: Some(1),
            height: Some(2),
            color_space: None,
            mtime: Some(1_700_000_000),
            content_hash: None,
        }
    }

    #[test]
    fn round_trip_raw_bytes_and_uid_unique() {
        let c = Catalog::open_in_memory().unwrap();
        c.insert_catalogue("cat-1", "2026-10-10T00:00:00Z").unwrap();
        let row = c.catalogue().unwrap().unwrap();
        assert_eq!(row.id, "cat-1");
        assert_eq!(row.generation, 0);
        let p = vec![0x8a, 0xff, 0x00, 0xc4, b':', 0xfe];
        let n = vec![0xff, 0xfe, 0x80];
        let id = c
            .insert_asset(&asset("u1", Some(p.clone()), Some(n.clone())))
            .unwrap();
        let got = c.asset(id).unwrap().unwrap();
        assert_eq!(got.path_raw, Some(p));
        assert_eq!(got.name_raw, Some(n));
        assert_eq!(got.mtime, Some(1_700_000_000));
        assert!(c.insert_asset(&asset("u1", None, None)).is_err());
        assert!(c.asset(9999).unwrap().is_none());
    }

    #[test]
    fn adversarial_bytes() {
        let c = Catalog::open_in_memory().unwrap();
        let big: Vec<u8> = (0..=255u8).cycle().take(100_000).collect();
        for (i, b) in [vec![], vec![0], big].into_iter().enumerate() {
            let id = c
                .insert_asset(&asset(&format!("u{i}"), Some(b.clone()), Some(b.clone())))
                .unwrap();
            assert_eq!(c.asset(id).unwrap().unwrap().path_raw, Some(b));
        }
    }

    #[test]
    fn migrations_idempotent() {
        let dir = std::env::temp_dir().join(format!("icat-mig-{}", std::process::id()));
        let _ = std::fs::remove_file(&dir);
        {
            let c = Catalog::open(&dir).unwrap();
            assert_eq!(c.schema_version().unwrap(), latest_version());
        }
        let c = Catalog::open(&dir).unwrap();
        assert_eq!(c.schema_version().unwrap(), latest_version());
        let n: i64 = c
            .connection()
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, latest_version());
        drop(c);
        let _ = std::fs::remove_file(&dir);
    }

    #[test]
    fn newer_version_refused() {
        let dir = std::env::temp_dir().join(format!("icat-new-{}", std::process::id()));
        let _ = std::fs::remove_file(&dir);
        {
            let c = Catalog::open(&dir).unwrap();
            c.connection()
                .execute(
                    "INSERT INTO schema_migrations (version, applied_utc) VALUES (?1, 'x')",
                    params![latest_version() + 1],
                )
                .unwrap();
        }
        match Catalog::open(&dir) {
            Err(CatalogError::NewerSchema { .. }) => {}
            _ => panic!("expected NewerSchema"),
        }
        let _ = std::fs::remove_file(&dir);
    }
}
