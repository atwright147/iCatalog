//! Shared verified download and replica-baseline preparation for sync/clone.
use crate::{
    delta,
    protocol::{HEADER_GENERATION, HEADER_ID, MAX_SNAPSHOT_BYTES},
    transport::{HttpResponse, Transport},
    util, Result, SyncError,
};
use std::io::Write;
use std::path::Path;

pub(crate) struct PreparedDownload {
    pub temp: util::TempPath,
    pub generation: i64,
    pub bytes: u64,
}

fn header_generation(resp: &HttpResponse) -> Result<i64> {
    resp.header(HEADER_GENERATION)
        .and_then(|s| s.parse::<i64>().ok())
        .filter(|g| *g >= 0)
        .ok_or_else(|| SyncError::Protocol("missing or invalid X-Generation".into()))
}

pub(crate) fn prepare(
    transport: &dyn Transport,
    url: &str,
    destination: &Path,
    expect_id: &str,
    minimum_generation: i64,
) -> Result<PreparedDownload> {
    let resp = transport.get(url)?;
    if resp.status != 200 {
        return Err(resp.status_error("GET /catalogue"));
    }
    if resp.body.len() as u64 > MAX_SNAPSHOT_BYTES {
        return Err(SyncError::Protocol(
            "catalogue download exceeds the snapshot limit".into(),
        ));
    }
    if resp.header(HEADER_ID) != Some(expect_id) {
        return Err(SyncError::Protocol("download catalogue id mismatch".into()));
    }
    let generation = header_generation(&resp)?;
    if generation < minimum_generation {
        return Err(SyncError::Protocol(
            "download is older than the acknowledged server state".into(),
        ));
    }
    let temp = util::TempPath::reserve(destination, "download")?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(temp.path())?;
    file.write_all(&resp.body)?;
    file.sync_all()?;
    drop(file);
    util::verify_catalogue(temp.path(), expect_id)?;
    {
        let c = icatalog_catalog::Catalog::open(temp.path())?;
        let mode: String = c
            .connection()
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .map_err(icatalog_catalog::CatalogError::from)?;
        if mode != "delete" {
            return Err(SyncError::Integrity(
                "downloaded catalogue must use SQLite DELETE journal mode".into(),
            ));
        }
        if c.generation()? != Some(generation) {
            return Err(SyncError::Integrity(
                "snapshot header and catalogue generation differ".into(),
            ));
        }
        if c.is_replica()? {
            return Err(SyncError::Integrity(
                "server sent a replica instead of an authoritative catalogue".into(),
            ));
        }
        let state = delta::encode(expect_id, &delta::current(&c)?)?;
        c.set_replica_base(generation, &state)?;
    }
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(temp.path())?
        .sync_all()?;
    Ok(PreparedDownload {
        temp,
        generation,
        bytes: resp.body.len() as u64,
    })
}
