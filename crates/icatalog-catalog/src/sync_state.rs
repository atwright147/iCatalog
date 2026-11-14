//! Per-replica companion SQLite state. It survives atomic replacement of the
//! catalogue itself and is bound to that catalogue's stable ID.
use crate::{CatalogError, Result};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use std::path::{Path, PathBuf};

pub struct SyncStateDb {
    path: PathBuf,
    conn: Connection,
}

impl SyncStateDb {
    pub fn open(catalogue_path: &Path, catalogue_id: &str, create: bool) -> Result<Self> {
        let state_path = companion_path(catalogue_path)?;
        match std::fs::symlink_metadata(&state_path) {
            Ok(metadata) if !metadata.file_type().is_file() => {
                return Err(CatalogError::InvalidQuery(
                    "sync state must be a regular file",
                ));
            }
            Ok(_metadata) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    if _metadata.nlink() > 1 {
                        return Err(CatalogError::InvalidQuery(
                            "sync state cannot have hard links",
                        ));
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && !create => {
                return Err(CatalogError::InvalidQuery("sync state does not exist"));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                match std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&state_path)
                {
                    Ok(file) => drop(file),
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(_) => {
                        return Err(CatalogError::InvalidQuery("cannot create sync state file"))
                    }
                }
            }
            Err(_) => return Err(CatalogError::InvalidQuery("cannot inspect sync state file")),
        }
        let flags = if create {
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX
        } else {
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX
        };
        let conn = Connection::open_with_flags(&state_path, flags)?;
        if create {
            conn.execute_batch("PRAGMA synchronous=FULL")?;
            let has_tables: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table')",
                [],
                |r| r.get(0),
            )?;
            if has_tables {
                let has_identity: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='state_meta')", [], |r| r.get(0))?;
                let has_state: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='state')", [], |r| r.get(0))?;
                if !has_identity || !has_state {
                    return Err(CatalogError::InvalidQuery("invalid sync state file"));
                }
                let existing: String = conn
                    .query_row(
                        "SELECT CASE WHEN length(value)<=1024 THEN value END FROM state_meta WHERE key='catalogue-id'",
                        [],
                        |r| r.get(0),
                    )
                    .map_err(|_| CatalogError::InvalidQuery("invalid sync state file"))?;
                if existing != catalogue_id {
                    return Err(CatalogError::InvalidQuery(
                        "sync state belongs to another catalogue",
                    ));
                }
            } else {
                let tx = conn.unchecked_transaction()?;
                tx.execute_batch("CREATE TABLE state_meta(key TEXT PRIMARY KEY,value TEXT NOT NULL); CREATE TABLE state(key TEXT PRIMARY KEY,value INTEGER NOT NULL);")?;
                tx.execute(
                    "INSERT INTO state_meta(key,value) VALUES('catalogue-id',?1)",
                    [catalogue_id],
                )?;
                tx.commit()?;
            }
        } else {
            let existing: String = conn
                .query_row(
                    "SELECT CASE WHEN length(value)<=1024 THEN value END FROM state_meta WHERE key='catalogue-id'",
                    [],
                    |r| r.get(0),
                )
                .map_err(|_| CatalogError::InvalidQuery("invalid sync state file"))?;
            if existing != catalogue_id {
                return Err(CatalogError::InvalidQuery(
                    "sync state belongs to another catalogue",
                ));
            }
            let has_state: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='state')", [], |r| r.get(0)).map_err(|_| CatalogError::InvalidQuery("invalid sync state file"))?;
            if !has_state {
                return Err(CatalogError::InvalidQuery("invalid sync state file"));
            }
        }
        Ok(Self {
            path: state_path,
            conn,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn connection(&self) -> &Connection {
        &self.conn
    }
    pub fn connection_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }
    pub fn into_connection(self) -> Connection {
        self.conn
    }

    pub fn required(&self) -> Result<bool> {
        let value: Option<i64> = self
            .conn
            .query_row(
                "SELECT value FROM state WHERE key='sync-required'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        Ok(value.unwrap_or(0) != 0)
    }

    pub fn set_required(&self, required: bool) -> Result<()> {
        self.conn.execute_batch("PRAGMA synchronous=FULL")?;
        self.conn.execute("INSERT INTO state(key,value) VALUES('sync-required',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [i64::from(required)])?;
        Ok(())
    }
}

pub fn companion_path(catalogue_path: &Path) -> Result<PathBuf> {
    if catalogue_path == Path::new(":memory:") || catalogue_path.as_os_str().is_empty() {
        return Err(CatalogError::InvalidQuery(
            "sync state requires an on-disk catalogue",
        ));
    }
    let mut name = catalogue_path.as_os_str().to_os_string();
    name.push(".sync-state.sqlite");
    Ok(PathBuf::from(name))
}
