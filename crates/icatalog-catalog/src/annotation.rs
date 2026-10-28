use rusqlite::{params, OptionalExtension};

use crate::{Annotation, Catalog, CatalogError, Collection, Keyword, Result};

impl Catalog {
    /// Manual and Smart Collections, ordered by name and then id.
    pub fn all_collections(&self) -> Result<Vec<Collection>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name FROM collection ORDER BY name, id")?;
        let rows = stmt.query_map([], |r| {
            Ok(Collection {
                id: r.get(0)?,
                name: r.get(1)?,
                rules: None,
            })
        })?;
        let mut collections = rows.collect::<std::result::Result<Vec<_>, _>>()?;
        for collection in &mut collections {
            collection.rules = self.smart_rules(collection.id)?;
        }
        Ok(collections)
    }

    /// Number of assets in each collection, including empty collections.
    pub fn collection_asset_counts(&self) -> Result<Vec<(i64, i64)>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT c.id, CASE WHEN sc.collection_id IS NULL THEN
             (SELECT COUNT(*) FROM collection_asset ca WHERE ca.collection_id=c.id)
             ELSE (SELECT COUNT(*) FROM asset WHERE {}) END
             FROM collection c LEFT JOIN smart_collection sc ON sc.collection_id=c.id
             ORDER BY c.name, c.id",
            crate::smart::SMART_MATCH
        ))?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// Collections containing an asset, ordered by collection id.
    pub fn asset_collection_ids(&self, asset_id: i64) -> Result<Vec<i64>> {
        self.id_list(&format!("SELECT c.id FROM collection c LEFT JOIN smart_collection sc ON sc.collection_id=c.id
            JOIN asset ON asset.id=?1 WHERE {} ORDER BY c.id", crate::smart::membership_sql()), asset_id)
    }

    /// Collection memberships for selected assets as `(asset_id, collection_id)`
    /// pairs. Duplicate input ids are ignored; at most 32,000 ids are supported.
    pub fn collection_memberships(&self, asset_ids: &[i64]) -> Result<Vec<(i64, i64)>> {
        if asset_ids.len() > 32_000 {
            return Err(CatalogError::InvalidQuery(
                "select at most 32000 assets at once",
            ));
        }
        if asset_ids.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<_> = asset_ids
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let marks = vec!["?"; ids.len()].join(",");
        let mut statement = self.conn.prepare(&format!(
            "SELECT asset.id, c.id FROM asset CROSS JOIN collection c
             LEFT JOIN smart_collection sc ON sc.collection_id=c.id
             WHERE asset.id IN ({marks}) AND {} ORDER BY asset.id, c.id",
            crate::smart::membership_sql()
        ))?;
        let rows = statement.query_map(rusqlite::params_from_iter(&ids), |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn insert_collection(&self, name: &str) -> Result<i64> {
        self.conn
            .execute("INSERT INTO collection (name) VALUES (?1)", params![name])?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn collection(&self, id: i64) -> Result<Option<Collection>> {
        let mut collection = self
            .conn
            .query_row(
                "SELECT id, name FROM collection WHERE id = ?1",
                params![id],
                |r| {
                    Ok(Collection {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        rules: None,
                    })
                },
            )
            .optional()?;
        if let Some(collection) = &mut collection {
            collection.rules = self.smart_rules(collection.id)?;
        }
        Ok(collection)
    }

    pub fn add_to_collection(&self, collection_id: i64, asset_id: i64) -> Result<()> {
        self.require_manual_collection(collection_id)?;
        self.conn.execute(
            "INSERT OR IGNORE INTO collection_asset (collection_id, asset_id) VALUES (?1, ?2)",
            params![collection_id, asset_id],
        )?;
        Ok(())
    }

    pub fn remove_from_collection(&self, collection_id: i64, asset_id: i64) -> Result<()> {
        self.require_manual_collection(collection_id)?;
        self.conn.execute(
            "DELETE FROM collection_asset WHERE collection_id = ?1 AND asset_id = ?2",
            params![collection_id, asset_id],
        )?;
        Ok(())
    }

    pub fn collection_assets(&self, collection_id: i64) -> Result<Vec<i64>> {
        self.id_list(
            &format!(
                "SELECT asset.id FROM asset CROSS JOIN collection c
            LEFT JOIN smart_collection sc ON sc.collection_id=c.id
            WHERE c.id=?1 AND {} ORDER BY asset.id",
                crate::smart::membership_sql()
            ),
            collection_id,
        )
    }

    fn require_manual_collection(&self, id: i64) -> Result<()> {
        if self.smart_rules(id)?.is_some() {
            return Err(CatalogError::InvalidQuery(
                "Smart Collection membership is determined by its rules",
            ));
        }
        Ok(())
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

    /// Set the generation counter to an exact value (the sync client records a
    /// server generation it has caught up to).
    pub fn set_generation(&self, generation: i64) -> Result<()> {
        let changed = self
            .conn
            .execute("UPDATE catalogue SET generation = ?1", params![generation])?;
        if changed == 0 {
            return Err(CatalogError::Sqlite(rusqlite::Error::QueryReturnedNoRows));
        }
        Ok(())
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
