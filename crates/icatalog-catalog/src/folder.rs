use rusqlite::{params, OptionalExtension};

use crate::{Catalog, Folder, Result};

const FOLDER_COLUMNS: &str = "id, parent_id, source_id, path, name";

fn folder_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Folder> {
    Ok(Folder {
        id: r.get(0)?,
        parent_id: r.get(1)?,
        source_id: r.get(2)?,
        path: r.get(3)?,
        name: r.get(4)?,
    })
}

impl Catalog {
    /// Insert a folder that belongs to no source (legacy or manual grouping).
    pub fn insert_folder(&self, parent_id: Option<i64>, name: &str) -> Result<i64> {
        self.insert_folder_at(parent_id, None, name, None)
    }

    /// Insert a folder, optionally rooted in an import source with a path
    /// relative to that source's root.
    pub fn insert_folder_at(
        &self,
        parent_id: Option<i64>,
        source_id: Option<i64>,
        name: &str,
        path: Option<&str>,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO folder (parent_id, source_id, path, name) VALUES (?1, ?2, ?3, ?4)",
            params![parent_id, source_id, path, name],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn folder(&self, id: i64) -> Result<Option<Folder>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {FOLDER_COLUMNS} FROM folder WHERE id = ?1"),
                params![id],
                folder_from_row,
            )
            .optional()?)
    }

    /// Find the folder with this path inside a source (`""` is the root).
    pub fn folder_by_source_path(&self, source_id: i64, path: &str) -> Result<Option<Folder>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {FOLDER_COLUMNS} FROM folder WHERE source_id = ?1 AND path = ?2"),
                params![source_id, path],
                folder_from_row,
            )
            .optional()?)
    }

    pub fn child_folders(&self, parent_id: Option<i64>) -> Result<Vec<Folder>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {FOLDER_COLUMNS} FROM folder WHERE parent_id IS ?1 ORDER BY id"
        ))?;
        let rows = stmt.query_map(params![parent_id], folder_from_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn delete_asset(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM asset WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// List all folders in the catalogue, ordered by id.
    pub fn all_folders(&self) -> Result<Vec<Folder>> {
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT {FOLDER_COLUMNS} FROM folder ORDER BY id"))?;
        let rows = stmt.query_map([], folder_from_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// List the folders belonging to one source, ordered by path.
    pub fn folders_in_source(&self, source_id: i64) -> Result<Vec<Folder>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {FOLDER_COLUMNS} FROM folder WHERE source_id = ?1 ORDER BY path"
        ))?;
        let rows = stmt.query_map(params![source_id], folder_from_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }
}
