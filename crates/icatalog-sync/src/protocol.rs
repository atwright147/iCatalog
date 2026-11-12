//! Protocol v2 discovery and catalogue download headers (see `docs/sync.md`).

use crate::error::{Result, SyncError};
use icatalog_catalog::valid_catalogue_id;

/// `GET /info` — catalogue id and generation.
pub const PATH_INFO: &str = "/info";
/// `GET /catalogue` — server snapshot; uploads are retired.
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
        format!(
            "protocol=2\nid={}\ngeneration={}\n",
            self.id, self.generation
        )
    }

    /// Parse the `GET /info` body leniently; unknown lines are ignored.
    pub fn parse(body: &str) -> Result<Self> {
        let mut id = None;
        let mut generation = None;
        let mut protocol = None;
        for line in body.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                return Err(SyncError::Protocol(
                    "malformed info line; expected key=value".into(),
                ));
            };
            match key.trim() {
                "protocol" => protocol = Some(value.trim()),
                "id" => id = Some(value.to_string()),
                "generation" => {
                    generation = Some(value.trim().parse::<i64>().map_err(|_| {
                        SyncError::Protocol("info generation must be an integer".into())
                    })?)
                }
                _ => {}
            }
        }
        let id = id.ok_or_else(|| SyncError::Protocol("info is missing 'id'".into()))?;
        if !valid_catalogue_id(&id) {
            return Err(SyncError::Protocol(
                "info has an invalid catalogue id".into(),
            ));
        }
        let generation =
            generation.ok_or_else(|| SyncError::Protocol("info is missing 'generation'".into()))?;
        if protocol != Some("2") || generation < 0 {
            return Err(SyncError::Protocol(
                "server must support sync protocol 2 with a valid generation; upgrade the server"
                    .into(),
            ));
        }
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::CatalogueInfo;

    #[test]
    fn info_id_round_trips_without_normalisation() {
        let info = CatalogueInfo {
            id: "Family Photos".into(),
            generation: 2,
        };
        assert_eq!(CatalogueInfo::parse(&info.encode()).unwrap(), info);
        for id in ["", " leading", "trailing ", "line\nbreak", "nonascii-é"] {
            let body = format!("protocol=2\nid={id}\ngeneration=2\n");
            assert!(CatalogueInfo::parse(&body).is_err(), "accepted {id:?}");
        }
    }
}
