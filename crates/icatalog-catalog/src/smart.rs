//! Bounded, non-recursive Smart Collection rules; all present rules are ANDed.
use crate::{Catalog, CatalogError, Result};
use rusqlite::{params, OptionalExtension};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SmartRules {
    /// Exact format, ignoring ASCII case (for example JPEG or NEF).
    pub format: Option<String>,
    /// Inclusive local capture dates, YYYY-MM-DD. Missing dates do not match.
    pub captured_from: Option<String>,
    pub captured_to: Option<String>,
    /// Case-insensitive substring of camera make/model or lens.
    pub camera: Option<String>,
    pub lens: Option<String>,
    pub min_rating: Option<i64>,
    pub label: Option<i64>,
    /// Exact keyword name, as in AssetQuery.
    pub keyword: Option<String>,
    /// Case-insensitive substring of filename or path.
    pub text: Option<String>,
}

impl SmartRules {
    pub fn validate(&self) -> Result<()> {
        let texts = [
            &self.format,
            &self.captured_from,
            &self.captured_to,
            &self.camera,
            &self.lens,
            &self.keyword,
            &self.text,
        ];
        if texts.iter().all(|t| t.is_none()) && self.min_rating.is_none() && self.label.is_none() {
            return Err(CatalogError::InvalidQuery(
                "a Smart Collection needs at least one rule",
            ));
        }
        for text in texts.into_iter().flatten() {
            if text.trim().is_empty() || text.len() > 1024 || text.chars().any(char::is_control) {
                return Err(CatalogError::InvalidQuery(
                    "rule text must be 1-1024 bytes without control characters",
                ));
            }
        }
        if self.min_rating.is_some_and(|v| !(0..=5).contains(&v))
            || self.label.is_some_and(|v| !(0..=7).contains(&v))
        {
            return Err(CatalogError::InvalidQuery(
                "rating must be 0-5 and label must be 0-7",
            ));
        }
        for date in [&self.captured_from, &self.captured_to]
            .into_iter()
            .flatten()
        {
            if !valid_date(date) {
                return Err(CatalogError::InvalidQuery(
                    "capture dates must be real dates in YYYY-MM-DD form",
                ));
            }
        }
        if matches!((&self.captured_from, &self.captured_to), (Some(from), Some(to)) if from > to) {
            return Err(CatalogError::InvalidQuery(
                "capture start date must not follow end date",
            ));
        }
        Ok(())
    }
}

fn valid_date(value: &str) -> bool {
    let b = value.as_bytes();
    if b.len() != 10 || b.get(4) != Some(&b'-') || b.get(7) != Some(&b'-') {
        return false;
    }
    let parse =
        |range: std::ops::Range<usize>| value.get(range).and_then(|s| s.parse::<u32>().ok());
    let (Some(year), Some(month), Some(day)) = (parse(0..4), parse(5..7), parse(8..10)) else {
        return false;
    };
    if !b
        .iter()
        .enumerate()
        .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
        || year == 0
    {
        return false;
    }
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&day)
}

// These expressions only interpolate trusted SQL. User rule values remain in
// typed columns; no stored SQL, recursion or per-collection parameter expansion.
pub(crate) const SMART_MATCH: &str = "
 (sc.format IS NULL OR lower(asset.format) = lower(sc.format))
 AND (sc.captured_from IS NULL OR EXISTS (SELECT 1 FROM asset_meta m WHERE m.asset_id=asset.id AND substr(m.captured_local,1,10)>=sc.captured_from))
 AND (sc.captured_to IS NULL OR EXISTS (SELECT 1 FROM asset_meta m WHERE m.asset_id=asset.id AND substr(m.captured_local,1,10)<=sc.captured_to))
 AND (sc.camera IS NULL OR EXISTS (SELECT 1 FROM asset_meta m WHERE m.asset_id=asset.id AND instr(lower(coalesce(m.make,'') || ' ' || coalesce(m.model,'')),lower(sc.camera))>0))
 AND (sc.lens IS NULL OR EXISTS (SELECT 1 FROM asset_meta m WHERE m.asset_id=asset.id AND instr(lower(m.lens),lower(sc.lens))>0))
 AND (sc.min_rating IS NULL OR coalesce((SELECT rating FROM annotation WHERE asset_id=asset.id),0)>=sc.min_rating)
 AND (sc.label IS NULL OR coalesce((SELECT label FROM annotation WHERE asset_id=asset.id),0)=sc.label)
 AND (sc.keyword IS NULL OR EXISTS (SELECT 1 FROM asset_keyword ak JOIN keyword k ON k.id=ak.keyword_id WHERE ak.asset_id=asset.id AND k.name=sc.keyword))
 AND (sc.text IS NULL OR instr(lower(asset.name),lower(sc.text))>0 OR instr(lower(asset.path),lower(sc.text))>0)";

pub(crate) fn membership_sql() -> String {
    format!("((sc.collection_id IS NULL AND EXISTS (SELECT 1 FROM collection_asset ca WHERE ca.collection_id=c.id AND ca.asset_id=asset.id)) OR (sc.collection_id IS NOT NULL AND ({SMART_MATCH})))")
}

impl Catalog {
    /// Count an unsaved rule set using exactly the saved membership predicate.
    /// No temporary collections or membership rows are written.
    pub fn count_smart_matches(&self, rules: &SmartRules) -> Result<i64> {
        rules.validate()?;
        Ok(self.conn.query_row(
            &format!("WITH sc(format,captured_from,captured_to,camera,lens,min_rating,label,keyword,text)
                AS (VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9))
                SELECT COUNT(*) FROM asset CROSS JOIN sc WHERE {SMART_MATCH}"),
            params![rules.format,rules.captured_from,rules.captured_to,rules.camera,rules.lens,
                rules.min_rating,rules.label,rules.keyword,rules.text],
            |row| row.get(0),
        )?)
    }

    pub fn smart_rules(&self, id: i64) -> Result<Option<SmartRules>> {
        let rules = self.conn.query_row(
            "SELECT format,captured_from,captured_to,camera,lens,min_rating,label,keyword,text FROM smart_collection WHERE collection_id=?1",
            [id], |r| Ok(SmartRules {format:r.get(0)?,captured_from:r.get(1)?,captured_to:r.get(2)?,camera:r.get(3)?,lens:r.get(4)?,min_rating:r.get(5)?,label:r.get(6)?,keyword:r.get(7)?,text:r.get(8)?})
        ).optional()?;
        if let Some(rules) = &rules {
            rules.validate()?;
        }
        Ok(rules)
    }

    /// The caller's transaction groups this with collection creation.
    pub fn set_smart_rules(&self, id: i64, rules: &SmartRules) -> Result<()> {
        rules.validate()?;
        self.conn.execute("INSERT INTO smart_collection(collection_id,format,captured_from,captured_to,camera,lens,min_rating,label,keyword,text)
            VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
            ON CONFLICT(collection_id) DO UPDATE SET format=excluded.format,captured_from=excluded.captured_from,captured_to=excluded.captured_to,camera=excluded.camera,lens=excluded.lens,min_rating=excluded.min_rating,label=excluded.label,keyword=excluded.keyword,text=excluded.text",
            params![id,rules.format,rules.captured_from,rules.captured_to,rules.camera,rules.lens,rules.min_rating,rules.label,rules.keyword,rules.text])?;
        Ok(())
    }
}
