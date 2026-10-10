use rusqlite::{params, OptionalExtension};

use crate::{Catalog, Folder, Result};

impl Catalog {
    pub fn insert_folder(&self, parent_id: Option<i64>, name: &str) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO folder (parent_id, name) VALUES (?1, ?2)",
            params![parent_id, name],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn folder(&self, id: i64) -> Result<Option<Folder>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, parent_id, name FROM folder WHERE id = ?1",
                params![id],
                |r| {
                    Ok(Folder {
                        id: r.get(0)?,
                        parent_id: r.get(1)?,
                        name: r.get(2)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn child_folders(&self, parent_id: Option<i64>) -> Result<Vec<Folder>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, parent_id, name FROM folder WHERE parent_id IS ?1 ORDER BY id")?;
        let rows = stmt.query_map(params![parent_id], |r| {
            Ok(Folder {
                id: r.get(0)?,
                parent_id: r.get(1)?,
                name: r.get(2)?,
            })
        })?;
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
}
