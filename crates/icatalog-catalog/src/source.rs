//! Import sources (v4/v5, D21): where assets came from, and whether they are present.

use rusqlite::{params, OptionalExtension};

use crate::{device_of, Catalog, Result, Source};

const SOURCE_COLUMNS: &str =
    "id, kind, label, identity, last_mount_path, last_seen_utc, device_id, device_label";

impl Catalog {
    /// Insert a source and return its row id (`s.id` is ignored).
    pub fn insert_source(&self, s: &Source) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO source
                 (kind, label, identity, last_mount_path, last_seen_utc, device_id, device_label)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                s.kind,
                s.label,
                s.identity,
                s.last_mount_path,
                s.last_seen_utc,
                s.device_id,
                s.device_label
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Find a source by its stable identity.
    pub fn source_by_identity(&self, identity: &str) -> Result<Option<Source>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {SOURCE_COLUMNS} FROM source WHERE identity = ?1"),
                params![identity],
                source_from_row,
            )
            .optional()?)
    }

    pub fn source(&self, id: i64) -> Result<Option<Source>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {SOURCE_COLUMNS} FROM source WHERE id = ?1"),
                params![id],
                source_from_row,
            )
            .optional()?)
    }

    pub fn all_sources(&self) -> Result<Vec<Source>> {
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT {SOURCE_COLUMNS} FROM source ORDER BY id"))?;
        let rows = stmt.query_map([], source_from_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Return the existing source for `identity` (refreshing its label, kind,
    /// mount path, device and last-seen time), or insert a new one.
    ///
    /// Identity is the absolute import-root path for now (D21); re-importing
    /// from the same root therefore reuses the source and its folders.
    pub fn ensure_source(
        &self,
        kind: &str,
        label: &str,
        identity: &str,
        last_mount_path: &str,
        last_seen_utc: &str,
    ) -> Result<i64> {
        let (device_id, device_label) = device_of(last_mount_path);
        if let Some(s) = self.source_by_identity(identity)? {
            self.conn.execute(
                "UPDATE source
                 SET kind = ?2, label = ?3, last_mount_path = ?4, last_seen_utc = ?5,
                     device_id = ?6, device_label = ?7
                 WHERE id = ?1",
                params![
                    s.id,
                    kind,
                    label,
                    last_mount_path,
                    last_seen_utc,
                    device_id,
                    device_label
                ],
            )?;
            return Ok(s.id);
        }
        self.conn.execute(
            "INSERT INTO source
                 (kind, label, identity, last_mount_path, last_seen_utc, device_id, device_label)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                kind,
                label,
                identity,
                last_mount_path,
                last_seen_utc,
                device_id,
                device_label
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Record that a source was seen at `path` at `seen_utc`.
    pub fn touch_source(&self, id: i64, path: &str, seen_utc: &str) -> Result<()> {
        let (device_id, device_label) = device_of(path);
        self.conn.execute(
            "UPDATE source
             SET last_mount_path = ?2, last_seen_utc = ?3, device_id = ?4, device_label = ?5
             WHERE id = ?1",
            params![id, path, seen_utc, device_id, device_label],
        )?;
        Ok(())
    }
}

fn source_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Source> {
    Ok(Source {
        id: r.get(0)?,
        kind: r.get(1)?,
        label: r.get(2)?,
        identity: r.get(3)?,
        last_mount_path: r.get(4)?,
        last_seen_utc: r.get(5)?,
        device_id: r.get(6)?,
        device_label: r.get(7)?,
    })
}
