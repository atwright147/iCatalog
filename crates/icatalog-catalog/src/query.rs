//! Filtered asset queries and aggregate statistics.

use rusqlite::types::Value;

use crate::{asset_from_row, Asset, Catalog, Result};

/// Filters for [`Catalog::query_assets`]. All set fields are ANDed together.
#[derive(Debug, Clone, Default)]
pub struct AssetQuery {
    /// Case-insensitive substring matched against `name` OR `path`.
    pub text_like: Option<String>,
    /// Case-insensitive substring matched against `asset.name`.
    pub name_like: Option<String>,
    /// Case-insensitive substring matched against `asset.path`.
    pub path_like: Option<String>,
    /// Exact keyword name; the asset must have this keyword.
    pub keyword: Option<String>,
    /// Minimum rating, inclusive.
    pub min_rating: Option<i64>,
    /// Exact label.
    pub label: Option<i64>,
    /// Inclusive lower bound on `asset_meta.captured_local` (ISO-8601 string, lexical compare).
    pub captured_from: Option<String>,
    /// Inclusive upper bound on `asset_meta.captured_local`.
    pub captured_to: Option<String>,
    /// Max rows (negative treated as "no limit").
    pub limit: Option<i64>,
    /// Rows to skip (NULL/negative treated as 0).
    pub offset: Option<i64>,
}

/// Aggregate counts for a catalogue.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CatalogStats {
    pub assets: i64,
    pub folders: i64,
    pub collections: i64,
    pub keywords: i64,
    pub thumbnails: i64,
    pub generation: Option<i64>,
    pub schema_version: i64,
}

impl Catalog {
    /// List assets matching all set filters, ordered by id.
    pub fn query_assets(&self, q: &AssetQuery) -> Result<Vec<Asset>> {
        let mut sql = String::from(
            "SELECT id, uid, folder_id, path, path_raw, name, name_raw, format, file_size, width, height, color_space, mtime, content_hash
             FROM asset WHERE 1=1",
        );
        let mut args: Vec<Value> = Vec::new();
        if let Some(t) = &q.text_like {
            sql.push_str(
                " AND (instr(lower(name), lower(?)) > 0 OR instr(lower(path), lower(?)) > 0)",
            );
            args.push(Value::Text(t.clone()));
            args.push(Value::Text(t.clone()));
        }
        if let Some(t) = &q.name_like {
            sql.push_str(" AND instr(lower(name), lower(?)) > 0");
            args.push(Value::Text(t.clone()));
        }
        if let Some(t) = &q.path_like {
            sql.push_str(" AND instr(lower(path), lower(?)) > 0");
            args.push(Value::Text(t.clone()));
        }
        if let Some(k) = &q.keyword {
            sql.push_str(
                " AND EXISTS (SELECT 1 FROM asset_keyword ak JOIN keyword k ON k.id = ak.keyword_id WHERE ak.asset_id = asset.id AND k.name = ?)",
            );
            args.push(Value::Text(k.clone()));
        }
        if let Some(r) = q.min_rating {
            sql.push_str(
                " AND EXISTS (SELECT 1 FROM annotation a WHERE a.asset_id = asset.id AND a.rating >= ?)",
            );
            args.push(Value::Integer(r));
        }
        if let Some(l) = q.label {
            sql.push_str(
                " AND EXISTS (SELECT 1 FROM annotation a WHERE a.asset_id = asset.id AND a.label = ?)",
            );
            args.push(Value::Integer(l));
        }
        if let Some(d) = &q.captured_from {
            sql.push_str(
                " AND EXISTS (SELECT 1 FROM asset_meta m WHERE m.asset_id = asset.id AND m.captured_local >= ?)",
            );
            args.push(Value::Text(d.clone()));
        }
        if let Some(d) = &q.captured_to {
            sql.push_str(
                " AND EXISTS (SELECT 1 FROM asset_meta m WHERE m.asset_id = asset.id AND m.captured_local <= ?)",
            );
            args.push(Value::Text(d.clone()));
        }
        sql.push_str(" ORDER BY id");
        let limit = q.limit.filter(|n| *n >= 0);
        let offset = q.offset.filter(|o| *o > 0);
        match (limit, offset) {
            (Some(n), Some(o)) => {
                sql.push_str(" LIMIT ? OFFSET ?");
                args.push(Value::Integer(n));
                args.push(Value::Integer(o));
            }
            (Some(n), None) => {
                sql.push_str(" LIMIT ?");
                args.push(Value::Integer(n));
            }
            (None, Some(o)) => {
                sql.push_str(" LIMIT -1 OFFSET ?");
                args.push(Value::Integer(o));
            }
            (None, None) => {}
        }
        let mut stmt = self.connection().prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(args.iter()), asset_from_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Aggregate counts for the catalogue.
    pub fn stats(&self) -> Result<CatalogStats> {
        let conn = self.connection();
        Ok(CatalogStats {
            assets: conn.query_row("SELECT COUNT(*) FROM asset", [], |r| r.get(0))?,
            folders: conn.query_row("SELECT COUNT(*) FROM folder", [], |r| r.get(0))?,
            collections: conn.query_row("SELECT COUNT(*) FROM collection", [], |r| r.get(0))?,
            keywords: conn.query_row("SELECT COUNT(*) FROM keyword", [], |r| r.get(0))?,
            thumbnails: conn.query_row("SELECT COUNT(*) FROM thumbnail", [], |r| r.get(0))?,
            generation: self.generation()?,
            schema_version: self.schema_version()?,
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::{Annotation, AssetMeta, Thumbnail};

    fn mk(uid: &str, path: &str, name: &str) -> Asset {
        Asset {
            id: 0,
            uid: uid.into(),
            folder_id: None,
            path: path.into(),
            path_raw: None,
            name: name.into(),
            name_raw: None,
            format: None,
            file_size: None,
            width: None,
            height: None,
            color_space: None,
            mtime: None,
            content_hash: None,
        }
    }

    fn setup() -> (Catalog, Vec<i64>) {
        let c = Catalog::open_in_memory().unwrap();
        c.insert_catalogue("cat", "2026-01-01T00:00:00Z").unwrap();
        let ids = vec![
            c.insert_asset(&mk("u1", "pics/Beach/one.jpg", "one.jpg"))
                .unwrap(),
            c.insert_asset(&mk("u2", "pics/city/Two.png", "Two.png"))
                .unwrap(),
            c.insert_asset(&mk("u3", "docs/three.tif", "three.tif"))
                .unwrap(),
        ];
        (c, ids)
    }

    fn ids(v: Vec<Asset>) -> Vec<i64> {
        v.into_iter().map(|a| a.id).collect()
    }

    fn ann(asset_id: i64, rating: i64, label: i64) -> Annotation {
        Annotation {
            asset_id,
            rating,
            label,
            caption: None,
            headline: None,
            creator: None,
            copyright: None,
            updated_utc: None,
        }
    }

    fn meta(asset_id: i64, captured: Option<&str>) -> AssetMeta {
        AssetMeta {
            asset_id,
            captured_local: captured.map(String::from),
            tz_offset_min: None,
            make: None,
            model: None,
            lens: None,
            focal_length_mm: None,
            f_number: None,
            exposure_s: None,
            iso: None,
            gps_lat: None,
            gps_lon: None,
            orientation: None,
            duration_ms: None,
        }
    }

    #[test]
    fn no_filters_returns_all_ordered() {
        let (c, i) = setup();
        assert_eq!(ids(c.query_assets(&AssetQuery::default()).unwrap()), i);
    }

    #[test]
    fn text_name_path_filters() {
        let (c, i) = setup();
        let q = |f: fn(&mut AssetQuery)| {
            let mut q = AssetQuery::default();
            f(&mut q);
            ids(c.query_assets(&q).unwrap())
        };
        assert_eq!(q(|q| q.text_like = Some("TWO".into())), vec![i[1]]);
        assert_eq!(q(|q| q.text_like = Some("docs".into())), vec![i[2]]);
        assert!(q(|q| q.text_like = Some("zzz".into())).is_empty());
        assert_eq!(q(|q| q.name_like = Some(".jpg".into())), vec![i[0]]);
        assert!(q(|q| q.name_like = Some("docs".into())).is_empty());
        assert_eq!(q(|q| q.path_like = Some("beach".into())), vec![i[0]]);
        assert!(q(|q| q.path_like = Some("three.tif2".into())).is_empty());
    }

    #[test]
    fn keyword_filter() {
        let (c, i) = setup();
        let k = c.ensure_keyword("sunset", None).unwrap();
        c.add_asset_keyword(i[0], k).unwrap();
        c.add_asset_keyword(i[2], k).unwrap();
        let q = AssetQuery {
            keyword: Some("sunset".into()),
            ..Default::default()
        };
        assert_eq!(ids(c.query_assets(&q).unwrap()), vec![i[0], i[2]]);
        let q = AssetQuery {
            keyword: Some("nope".into()),
            ..Default::default()
        };
        assert!(c.query_assets(&q).unwrap().is_empty());
    }

    #[test]
    fn rating_and_label_filters() {
        let (c, i) = setup();
        c.set_annotation(&ann(i[0], 5, 2)).unwrap();
        c.set_annotation(&ann(i[1], 3, 7)).unwrap();
        let q = AssetQuery {
            min_rating: Some(4),
            ..Default::default()
        };
        assert_eq!(ids(c.query_assets(&q).unwrap()), vec![i[0]]);
        let q = AssetQuery {
            min_rating: Some(3),
            ..Default::default()
        };
        assert_eq!(ids(c.query_assets(&q).unwrap()), vec![i[0], i[1]]);
        let q = AssetQuery {
            label: Some(7),
            ..Default::default()
        };
        assert_eq!(ids(c.query_assets(&q).unwrap()), vec![i[1]]);
    }

    #[test]
    fn captured_range() {
        let (c, i) = setup();
        c.upsert_asset_meta(&meta(i[0], Some("2020-06-01T00:00:00Z")))
            .unwrap();
        c.upsert_asset_meta(&meta(i[1], Some("2021-06-01T00:00:00Z")))
            .unwrap();
        c.upsert_asset_meta(&meta(i[2], None)).unwrap();
        let q = AssetQuery {
            captured_from: Some("2020-06-01T00:00:00Z".into()),
            ..Default::default()
        };
        assert_eq!(ids(c.query_assets(&q).unwrap()), vec![i[0], i[1]]);
        let q = AssetQuery {
            captured_to: Some("2020-12-31T00:00:00Z".into()),
            ..Default::default()
        };
        assert_eq!(ids(c.query_assets(&q).unwrap()), vec![i[0]]);
        let q = AssetQuery {
            captured_from: Some("2021-01-01T00:00:00Z".into()),
            captured_to: Some("2021-12-31T00:00:00Z".into()),
            ..Default::default()
        };
        assert_eq!(ids(c.query_assets(&q).unwrap()), vec![i[1]]);
    }

    #[test]
    fn paging() {
        let (c, i) = setup();
        let run = |limit, offset| {
            ids(c
                .query_assets(&AssetQuery {
                    limit,
                    offset,
                    ..Default::default()
                })
                .unwrap())
        };
        assert_eq!(run(Some(2), None), vec![i[0], i[1]]);
        assert_eq!(run(Some(2), Some(1)), vec![i[1], i[2]]);
        assert_eq!(run(None, Some(2)), vec![i[2]]);
        assert_eq!(run(Some(-1), Some(-5)), i);
        assert!(run(Some(0), None).is_empty());
    }

    #[test]
    fn stats_counts() {
        let (c, i) = setup();
        c.insert_folder(None, "f").unwrap();
        c.insert_collection("col").unwrap();
        c.ensure_keyword("k", None).unwrap();
        c.insert_thumbnail(&Thumbnail {
            asset_id: i[0],
            size_class: 1,
            source: "s".into(),
            format: "jpeg".into(),
            width: 1,
            height: 1,
            orientation_applied: false,
            bytes: vec![1],
            derived_from: None,
            generator_version: 1,
        })
        .unwrap();
        let s = c.stats().unwrap();
        assert_eq!(s.assets, 3);
        assert_eq!(s.folders, 1);
        assert_eq!(s.collections, 1);
        assert_eq!(s.keywords, 1);
        assert_eq!(s.thumbnails, 1);
        assert_eq!(s.generation, Some(0));
        assert_eq!(s.schema_version, 2);
    }
}
