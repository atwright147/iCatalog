//! User-editable sync state; all compound writes use the caller's transaction.
use crate::{Annotation, Catalog, CatalogError, Result, SmartRules};
use rusqlite::{params, OptionalExtension};

pub const MAX_SYNC_ROWS: usize = 100_000;
pub const MAX_SYNC_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnnotationState {
    pub uid: String,
    pub revision: i64,
    pub rating: i64,
    pub label: i64,
    pub keywords: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionState {
    pub uid: String,
    pub revision: i64,
    pub name: String,
    pub rules: Option<SmartRules>,
    /// Stable asset uids, sorted; empty for Smart Collections.
    pub members: Vec<String>,
}

impl Catalog {
    pub fn is_replica(&self) -> Result<bool> {
        Ok(self
            .conn
            .query_row("SELECT EXISTS(SELECT 1 FROM replica_base)", [], |r| {
                r.get(0)
            })?)
    }

    pub fn replica_base(&self) -> Result<Option<(i64, Vec<u8>)>> {
        let size: Option<i64> = self
            .conn
            .query_row("SELECT length(data) FROM replica_base", [], |r| r.get(0))
            .optional()?;
        if size.is_some_and(|n| n < 0 || n > MAX_SYNC_BYTES as i64) {
            return Err(CatalogError::InvalidQuery(
                "replica baseline exceeds the sync limit",
            ));
        }
        Ok(self
            .conn
            .query_row("SELECT generation,data FROM replica_base", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?)
    }

    pub fn set_replica_base(&self, generation: i64, bytes: &[u8]) -> Result<()> {
        if generation < 0 || bytes.len() > MAX_SYNC_BYTES {
            return Err(CatalogError::InvalidQuery("invalid replica baseline"));
        }
        self.conn.execute("INSERT INTO replica_base VALUES(1,?1,?2) ON CONFLICT(singleton) DO UPDATE SET generation=excluded.generation,data=excluded.data", params![generation, bytes])?;
        Ok(())
    }

    pub fn annotation_states(&self) -> Result<Vec<AnnotationState>> {
        let mut stmt = self.conn.prepare("SELECT a.id,CASE WHEN length(CAST(a.uid AS BLOB))<=1024 THEN a.uid END,r.revision,COALESCE(n.rating,0),COALESCE(n.label,0) FROM asset a LEFT JOIN sync_revision r ON r.kind='asset' AND r.entity_id=a.id LEFT JOIN annotation n ON n.asset_id=a.id ORDER BY a.uid LIMIT 100001")?;
        let mut out = Vec::new();
        let mut size = 0usize;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                AnnotationState {
                    uid: r.get(1)?,
                    revision: r.get(2)?,
                    rating: r.get(3)?,
                    label: r.get(4)?,
                    keywords: Vec::new(),
                },
            ))
        })?;
        for row in rows {
            if out.len() >= MAX_SYNC_ROWS {
                return Err(CatalogError::InvalidQuery(
                    "sync supports at most 100000 assets",
                ));
            }
            let (id, mut state) = row?;
            add_size(&mut size, state.uid.len().saturating_add(64))?;
            let mut keywords = self.conn.prepare("SELECT CASE WHEN length(CAST(k.name AS BLOB))<=1048576 THEN k.name END FROM asset_keyword ak JOIN keyword k ON k.id=ak.keyword_id WHERE ak.asset_id=?1 ORDER BY k.name LIMIT 100001")?;
            let names = keywords.query_map([id], |r| r.get::<_, String>(0))?;
            for name in names {
                let name = name?;
                add_size(&mut size, name.len().saturating_add(16))?;
                if state.keywords.len() >= MAX_SYNC_ROWS {
                    return Err(CatalogError::InvalidQuery("too many keywords to sync"));
                }
                state.keywords.push(name);
            }
            out.push(state);
        }
        Ok(out)
    }

    pub fn collection_states(&self) -> Result<Vec<CollectionState>> {
        let mut stmt = self.conn.prepare("SELECT c.id,CASE WHEN length(CAST(c.uid AS BLOB))<=1024 THEN c.uid END,r.revision,CASE WHEN length(CAST(c.name AS BLOB))<=1024 THEN c.name END FROM collection c LEFT JOIN sync_revision r ON r.kind='collection' AND r.entity_id=c.id ORDER BY c.uid LIMIT 100001")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                CollectionState {
                    uid: r.get(1)?,
                    revision: r.get(2)?,
                    name: r.get(3)?,
                    rules: None,
                    members: Vec::new(),
                },
            ))
        })?;
        let mut out = Vec::new();
        let mut size = 0usize;
        for row in rows {
            if out.len() >= MAX_SYNC_ROWS {
                return Err(CatalogError::InvalidQuery(
                    "sync supports at most 100000 collections",
                ));
            }
            let (id, mut state) = row?;
            state.rules = self.smart_rules(id)?;
            add_size(
                &mut size,
                state
                    .uid
                    .len()
                    .saturating_add(state.name.len())
                    .saturating_add(128),
            )?;
            if let Some(r) = &state.rules {
                for s in [
                    &r.format,
                    &r.captured_from,
                    &r.captured_to,
                    &r.camera,
                    &r.lens,
                    &r.keyword,
                    &r.text,
                ]
                .into_iter()
                .flatten()
                {
                    add_size(&mut size, s.len())?;
                }
            }
            if state.rules.is_none() {
                let mut members = self.conn.prepare("SELECT CASE WHEN length(CAST(a.uid AS BLOB))<=1024 THEN a.uid END FROM collection_asset ca JOIN asset a ON a.id=ca.asset_id WHERE ca.collection_id=?1 ORDER BY a.uid LIMIT 100001")?;
                for uid in members.query_map([id], |r| r.get::<_, String>(0))? {
                    let uid = uid?;
                    add_size(&mut size, uid.len().saturating_add(16))?;
                    state.members.push(uid);
                }
                if state.members.len() > MAX_SYNC_ROWS {
                    return Err(CatalogError::InvalidQuery(
                        "collection exceeds the sync membership limit",
                    ));
                }
            }
            out.push(state);
        }
        Ok(out)
    }

    pub fn apply_annotation_state(&self, state: &AnnotationState, updated_utc: &str) -> Result<()> {
        let asset = self
            .asset_by_uid(&state.uid)?
            .ok_or(CatalogError::InvalidQuery(
                "annotation asset no longer exists",
            ))?;
        let mut a = self.annotation(asset.id)?.unwrap_or(Annotation {
            asset_id: asset.id,
            rating: 0,
            label: 0,
            caption: None,
            headline: None,
            creator: None,
            copyright: None,
            updated_utc: None,
        });
        a.rating = state.rating;
        a.label = state.label;
        a.updated_utc = Some(updated_utc.to_owned());
        self.set_annotation(&a)?;
        self.conn
            .execute("DELETE FROM asset_keyword WHERE asset_id=?1", [asset.id])?;
        for name in &state.keywords {
            let id = self.ensure_keyword(name, None)?;
            self.add_asset_keyword(asset.id, id)?;
        }
        Ok(())
    }

    pub fn collection_id_by_uid(&self, uid: &str) -> Result<Option<i64>> {
        Ok(self
            .conn
            .query_row("SELECT id FROM collection WHERE uid=?1", [uid], |r| {
                r.get(0)
            })
            .optional()?)
    }

    pub fn apply_collection_state(&self, state: &CollectionState) -> Result<()> {
        let id = match self.collection_id_by_uid(&state.uid)? {
            Some(id) => {
                self.rename_collection(id, &state.name)?;
                id
            }
            None => {
                self.conn.execute(
                    "INSERT INTO collection(name,uid) VALUES(?1,?2)",
                    params![state.name, state.uid],
                )?;
                self.conn.last_insert_rowid()
            }
        };
        self.conn
            .execute("DELETE FROM collection_asset WHERE collection_id=?1", [id])?;
        self.conn
            .execute("DELETE FROM smart_collection WHERE collection_id=?1", [id])?;
        if let Some(rules) = &state.rules {
            self.set_smart_rules(id, rules)?;
        } else {
            for uid in &state.members {
                let asset = self.asset_by_uid(uid)?.ok_or(CatalogError::InvalidQuery(
                    "collection member no longer exists",
                ))?;
                self.add_to_collection(id, asset.id)?;
            }
        }
        Ok(())
    }
}

fn add_size(size: &mut usize, bytes: usize) -> Result<()> {
    *size = size
        .checked_add(bytes)
        .ok_or(CatalogError::InvalidQuery("sync state too large"))?;
    if *size > MAX_SYNC_BYTES {
        return Err(CatalogError::InvalidQuery("sync state exceeds 64 MiB"));
    }
    Ok(())
}
