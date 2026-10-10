use rusqlite::{params, OptionalExtension};

use crate::{Annotation, Catalog, CatalogError, Collection, Keyword, Result};

impl Catalog {
    pub fn insert_collection(&self, name: &str) -> Result<i64> {
        self.conn
            .execute("INSERT INTO collection (name) VALUES (?1)", params![name])?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn collection(&self, id: i64) -> Result<Option<Collection>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, name FROM collection WHERE id = ?1",
                params![id],
                |r| {
                    Ok(Collection {
                        id: r.get(0)?,
                        name: r.get(1)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn add_to_collection(&self, collection_id: i64, asset_id: i64) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO collection_asset (collection_id, asset_id) VALUES (?1, ?2)",
            params![collection_id, asset_id],
        )?;
        Ok(())
    }

    pub fn remove_from_collection(&self, collection_id: i64, asset_id: i64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM collection_asset WHERE collection_id = ?1 AND asset_id = ?2",
            params![collection_id, asset_id],
        )?;
        Ok(())
    }

    pub fn collection_assets(&self, collection_id: i64) -> Result<Vec<i64>> {
        self.id_list(
            "SELECT asset_id FROM collection_asset WHERE collection_id = ?1 ORDER BY asset_id",
            collection_id,
        )
    }

    pub fn generation(&self) -> Result<Option<i64>> {
        Ok(self
            .conn
            .query_row("SELECT generation FROM catalogue LIMIT 1", [], |r| r.get(0))
            .optional()?)
    }

    pub fn bump_generation(&self) -> Result<i64> {
        let changed = self
            .conn
            .execute("UPDATE catalogue SET generation = generation + 1", [])?;
        if changed == 0 {
            return Err(CatalogError::Sqlite(rusqlite::Error::QueryReturnedNoRows));
        }
        self.generation()?
            .ok_or(CatalogError::Sqlite(rusqlite::Error::QueryReturnedNoRows))
    }

    pub fn ensure_keyword(&self, name: &str, name_raw: Option<&[u8]>) -> Result<i64> {
        self.conn.execute(
            "INSERT OR IGNORE INTO keyword (name, name_raw) VALUES (?1, ?2)",
            params![name, name_raw],
        )?;
        Ok(self.conn.query_row(
            "SELECT id FROM keyword WHERE name = ?1",
            params![name],
            |r| r.get(0),
        )?)
    }

    pub fn add_asset_keyword(&self, asset_id: i64, keyword_id: i64) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO asset_keyword (asset_id, keyword_id) VALUES (?1, ?2)",
            params![asset_id, keyword_id],
        )?;
        Ok(())
    }

    pub fn remove_asset_keyword(&self, asset_id: i64, keyword_id: i64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM asset_keyword WHERE asset_id = ?1 AND keyword_id = ?2",
            params![asset_id, keyword_id],
        )?;
        Ok(())
    }

    pub fn asset_keywords(&self, asset_id: i64) -> Result<Vec<Keyword>> {
        let mut stmt = self.conn.prepare(
            "SELECT k.id, k.name, k.name_raw FROM keyword k
             JOIN asset_keyword ak ON ak.keyword_id = k.id
             WHERE ak.asset_id = ?1 ORDER BY k.name",
        )?;
        let rows = stmt.query_map(params![asset_id], |r| {
            Ok(Keyword {
                id: r.get(0)?,
                name: r.get(1)?,
                name_raw: r.get(2)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn assets_for_keyword(&self, keyword_id: i64) -> Result<Vec<i64>> {
        self.id_list(
            "SELECT asset_id FROM asset_keyword WHERE keyword_id = ?1 ORDER BY asset_id",
            keyword_id,
        )
    }

    pub fn set_annotation(&self, a: &Annotation) -> Result<()> {
        self.conn.execute(
            "INSERT INTO annotation (asset_id, rating, label, caption, headline, creator, copyright, updated_utc)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(asset_id) DO UPDATE SET
               rating = excluded.rating,
               label = excluded.label,
               caption = excluded.caption,
               headline = excluded.headline,
               creator = excluded.creator,
               copyright = excluded.copyright,
               updated_utc = excluded.updated_utc",
            params![
                a.asset_id,
                a.rating,
                a.label,
                a.caption,
                a.headline,
                a.creator,
                a.copyright,
                a.updated_utc
            ],
        )?;
        Ok(())
    }

    pub fn annotation(&self, asset_id: i64) -> Result<Option<Annotation>> {
        Ok(self
            .conn
            .query_row(
                "SELECT asset_id, rating, label, caption, headline, creator, copyright, updated_utc
                 FROM annotation WHERE asset_id = ?1",
                params![asset_id],
                |r| {
                    Ok(Annotation {
                        asset_id: r.get(0)?,
                        rating: r.get(1)?,
                        label: r.get(2)?,
                        caption: r.get(3)?,
                        headline: r.get(4)?,
                        creator: r.get(5)?,
                        copyright: r.get(6)?,
                        updated_utc: r.get(7)?,
                    })
                },
            )
            .optional()?)
    }

    fn id_list(&self, sql: &str, arg: i64) -> Result<Vec<i64>> {
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(params![arg], |r| r.get::<_, i64>(0))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }
}
