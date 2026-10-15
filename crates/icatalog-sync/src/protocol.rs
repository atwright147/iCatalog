//! Wire protocol for whole-snapshot sync (see `docs/sync.md`).

use crate::error::{Result, SyncError};

/// `GET /info` — catalogue id and generation.
pub const PATH_INFO: &str = "/info";
/// `GET`/`POST /catalogue` — the snapshot.
pub const PATH_CATALOGUE: &str = "/catalogue";
/// Response header carrying the catalogue id.
pub const HEADER_ID: &str = "X-Catalogue-Id";
/// Request/response header carrying a generation counter.
pub const HEADER_GENERATION: &str = "X-Generation";

/// Hard cap on a snapshot body, so a hostile or broken peer cannot exhaust
/// memory. Whole-snapshot transfer is the first slice (docs/sync.md section 3).
pub const MAX_SNAPSHOT_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// The catalogue id and generation reported by `GET /info`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogueInfo {
    pub id: String,
    pub generation: i64,
}

impl CatalogueInfo {
    /// Encode as `key=value` lines.
    pub fn encode(&self) -> String {
        format!("id={}\ngeneration={}\n", self.id, self.generation)
    }

    /// Parse the `GET /info` body leniently; unknown lines are ignored.
    pub fn parse(body: &str) -> Result<Self> {
        let mut id = None;
        let mut generation = None;
        for line in body.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                return Err(SyncError::Protocol(format!("malformed info line '{line}'")));
            };
            match key.trim() {
                "id" => id = Some(value.trim().to_string()),
                "generation" => {
                    generation = Some(value.trim().parse::<i64>().map_err(|_| {
                        SyncError::Protocol(format!("bad generation '{}'", value.trim()))
                    })?)
                }
                _ => {}
            }
        }
        let id = id.ok_or_else(|| SyncError::Protocol("info is missing 'id'".into()))?;
        if id.is_empty() {
            return Err(SyncError::Protocol("info has an empty id".into()));
        }
        let generation =
            generation.ok_or_else(|| SyncError::Protocol("info is missing 'generation'".into()))?;
        Ok(CatalogueInfo { id, generation })
    }
}

/// Split a request target into its path and query.
pub fn split_url(url: &str) -> (&str, &str) {
    match url.split_once('?') {
        Some((path, query)) => (path, query),
        None => (url, ""),
    }
}

/// Read a `key=value` query parameter (first match).
pub fn query_param<'a>(query: &'a str, name: &str) -> Option<&'a str> {
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == name).then_some(value)
    })
}
