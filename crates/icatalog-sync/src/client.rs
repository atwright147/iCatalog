//! Push revision-checked user edits, then verify/back up/replace the replica.
use crate::{
    delta,
    protocol::{CatalogueInfo, PATH_CATALOGUE, PATH_INFO},
    transport::Transport,
    util, Result, SyncError,
};
use icatalog_catalog::CatalogError;
use icatalog_engine::Context;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncAction {
    UpToDate,
    Pushed,
    Pulled { backup: Option<PathBuf> },
    Resynced { backup: Option<PathBuf> },
}
#[derive(Debug, Clone)]
pub struct SyncReport {
    pub action: SyncAction,
    pub local_generation: i64,
    pub server_generation: i64,
    pub bytes: u64,
    pub sent_bytes: u64,
    pub conflicts: Vec<String>,
    pub backup: Option<PathBuf>,
    pub initial: bool,
}
fn join(base: &str, path: &str) -> String {
    format!("{}{}", base.trim_end_matches('/'), path)
}
pub fn info(t: &dyn Transport, base: &str) -> Result<CatalogueInfo> {
    let resp = t.get(&join(base, PATH_INFO))?;
    if resp.status != 200 || resp.body.len() > 8192 {
        return Err(SyncError::Protocol("invalid GET /info response".into()));
    }
    CatalogueInfo::parse(
        std::str::from_utf8(&resp.body)
            .map_err(|_| SyncError::Protocol("info is not UTF-8".into()))?,
    )
}
/// The SQLite write reservation prevents another local writer from changing
/// the state being uploaded/backed up while network requests are in flight.
pub fn sync_once(ctx: &mut Context, t: &dyn Transport, base: &str) -> Result<SyncReport> {
    let mode: String = ctx
        .catalog()
        .connection()
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .map_err(CatalogError::from)?;
    if mode != "delete" {
        return Err(SyncError::Protocol(
            "sync requires SQLite DELETE journal mode".into(),
        ));
    }
    ctx.catalog()
        .connection()
        .execute_batch("BEGIN IMMEDIATE")
        .map_err(CatalogError::from)?;
    let result = sync_locked(ctx, t, base);
    if !ctx.catalog().connection().is_autocommit() {
        ctx.catalog()
            .connection()
            .execute_batch("ROLLBACK")
            .map_err(CatalogError::from)?;
    }
    result
}
fn sync_locked(ctx: &mut Context, t: &dyn Transport, base: &str) -> Result<SyncReport> {
    let row = ctx
        .catalog()
        .catalogue()?
        .ok_or_else(|| SyncError::Protocol("local catalogue is not initialised".into()))?;
    let remote = info(t, base)?;
    if remote.id != row.id {
        return Err(SyncError::Protocol(
            "server catalogue id does not match the local catalogue".into(),
        ));
    }
    let baseline = ctx.catalog().replica_base()?;
    let mut changes = Vec::new();
    let base_generation = if let Some((generation, bytes)) = &baseline {
        let (id, previous) = delta::decode(bytes)?;
        if id != row.id {
            return Err(SyncError::Integrity(
                "replica baseline belongs to another catalogue".into(),
            ));
        }
        let old: BTreeMap<_, _> = previous.into_iter().map(|r| (r.key(), r)).collect();
        let current = delta::current(ctx.catalog())?;
        let keys: BTreeSet<_> = current.iter().map(|r| r.key()).collect();
        if old.keys().any(|k| !keys.contains(k)) {
            return Err(SyncError::Protocol("local server-managed data was deleted; restore it or recover from a backup before syncing".into()));
        }
        for mut value in current {
            let previous = old.get(&value.key());
            if previous.is_none_or(|p| !p.same_value(&value)) {
                value.base_revision = previous.map(|p| p.revision());
                changes.push(value);
            }
        }
        *generation
    } else {
        0
    };
    if baseline.is_some() && remote.generation < base_generation {
        return Err(SyncError::Protocol(
            "server generation moved backwards; refusing to replace the local catalogue".into(),
        ));
    }
    if baseline.is_some() && remote.generation == base_generation && changes.is_empty() {
        return Ok(SyncReport {
            action: SyncAction::UpToDate,
            local_generation: base_generation,
            server_generation: remote.generation,
            bytes: 0,
            sent_bytes: 0,
            conflicts: Vec::new(),
            backup: None,
            initial: false,
        });
    }
    let mut sent_bytes = 0;
    let mut conflicts = Vec::new();
    let mut minimum_generation = remote.generation;
    if !changes.is_empty() {
        let body = delta::encode(&row.id, &changes)?;
        let resp = t.post(&join(base, "/annotations"), &body)?;
        if resp.status != 200 {
            return Err(SyncError::Http(format!(
                "POST /annotations returned {}",
                resp.status
            )));
        }
        let (generation, returned) = delta::decode_reply(&resp.body)?;
        if generation < remote.generation {
            return Err(SyncError::Protocol(
                "edit response generation moved backwards".into(),
            ));
        }
        let sent: BTreeSet<_> = changes.iter().map(|c| c.key()).collect();
        let mut seen = BTreeSet::new();
        if returned
            .iter()
            .any(|k| !sent.contains(k) || !seen.insert(k.clone()))
        {
            return Err(SyncError::Protocol(
                "invalid conflict list in edit response".into(),
            ));
        }
        sent_bytes = body.len() as u64;
        conflicts = returned;
        minimum_generation = generation;
    }
    let download = crate::download::prepare(
        t,
        &join(base, PATH_CATALOGUE),
        ctx.path(),
        &row.id,
        minimum_generation,
    )?;
    // No writes were made to the locked local catalogue. Its committed main
    // file is a consistent backup in DELETE journal mode.
    let backup = util::backup_local(ctx.path())?;
    ctx.replace_catalogue_file(download.temp.path())?;
    util::sync_parent(ctx.path())?;
    let action = if !conflicts.is_empty() {
        SyncAction::Resynced {
            backup: backup.clone(),
        }
    } else if sent_bytes > 0 {
        SyncAction::Pushed
    } else {
        SyncAction::Pulled {
            backup: backup.clone(),
        }
    };
    Ok(SyncReport {
        action,
        local_generation: base_generation,
        server_generation: download.generation,
        bytes: download.bytes,
        sent_bytes,
        conflicts,
        backup,
        initial: baseline.is_none(),
    })
}

/// On-demand, pull-only preview cache. Already verified local bytes need no HTTP request.
pub fn fetch_preview(ctx: &Context, t: &dyn Transport, base: &str, hash: &str) -> Result<bool> {
    if !icatalog_catalog::valid_preview_hash(hash) {
        return Err(SyncError::Protocol("invalid preview hash".into()));
    }
    if icatalog_engine::previews::read(ctx.catalog(), ctx.path(), hash)?.is_some() {
        return Ok(false);
    }
    if !ctx.catalog().is_replica()? || !ctx.catalog().preview_is_referenced(hash)? {
        return Err(SyncError::Protocol(
            "sync the catalogue before fetching a referenced preview".into(),
        ));
    }
    let response = t.get(&join(base, &format!("/preview/{hash}")))?;
    if response.status != 200 {
        return Err(SyncError::Http(format!(
            "preview GET returned {}",
            response.status
        )));
    }
    icatalog_engine::previews::cache(ctx.catalog(), ctx.path(), hash, &response.body)?;
    Ok(true)
}
