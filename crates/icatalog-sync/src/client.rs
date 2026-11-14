//! Push revision-checked user edits, then verify/back up/replace the replica.
use crate::{
    delta,
    protocol::{CatalogueInfo, PATH_CATALOGUE, PATH_INFO},
    transport::Transport,
    util, Result, SyncError,
};
use icatalog_catalog::CatalogError;
use icatalog_engine::Context;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
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
    if resp.status != 200 {
        return Err(resp.status_error("GET /info"));
    }
    if resp.body.len() > 8192 {
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
    sync_once_with_requirement(ctx, t, base, false)
}

pub fn sync_once_with_requirement(
    ctx: &mut Context,
    t: &dyn Transport,
    base: &str,
    require_replica: bool,
) -> Result<SyncReport> {
    recover_staged_snapshot(ctx)?;
    ctx.report_progress(icatalog_engine::SyncProgress {
        phase: icatalog_engine::SyncPhase::Preparing,
        completed: None,
        total: None,
    });
    ctx.ensure_replaceable_path()?;
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
    if require_replica {
        match ctx.catalog().is_replica() {
            Ok(true) => {}
            Ok(false) => {
                ctx.catalog()
                    .connection()
                    .execute_batch("ROLLBACK")
                    .map_err(CatalogError::from)?;
                return Err(SyncError::Protocol(
                    "this sync prompt requires an established replica; the catalogue is untracked"
                        .into(),
                ));
            }
            Err(error) => {
                let _ = ctx.catalog().connection().execute_batch("ROLLBACK");
                return Err(error.into());
            }
        }
    }
    let result =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sync_locked(ctx, t, base)))
            .unwrap_or_else(|_| {
                Err(SyncError::Protocol(
            "sync operation was interrupted; reopen the catalogue and inspect sync recovery state"
                .into(),
        ))
            });
    if !ctx.catalog().connection().is_autocommit() {
        ctx.catalog()
            .connection()
            .execute_batch("ROLLBACK")
            .map_err(CatalogError::from)?;
    }
    result
}

/// Finish or safely stop an interrupted snapshot swap before any command can
/// treat the local generation as the server baseline.
pub(crate) fn recover_staged_snapshot(ctx: &mut Context) -> Result<bool> {
    let Some(record) = crate::journal::staged_snapshot(ctx)? else {
        return Ok(false);
    };
    // Keep a second connection's SQLite writer reservation alive while the
    // Context connection is closed and the verified stage replaces the file.
    // `Context::replace_catalogue_file` must close its own handle before rename.
    ctx.ensure_replaceable_path()?;
    let recovery_guard = reserve_recovery_writer(ctx.path())?;
    let journal_mode: String = recovery_guard
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .map_err(CatalogError::from)?;
    if journal_mode != "delete" {
        return Err(SyncError::Protocol(
            "sync recovery requires SQLite DELETE journal mode; catalogue was left untouched"
                .into(),
        ));
    }
    let name = &record.staged_name;
    if !name.starts_with(".download.icatalog-sync-") || name.contains('/') || name.contains('\\') {
        return Err(SyncError::Integrity(
            "invalid staged snapshot name in sync recovery state".into(),
        ));
    }
    let parent = util::parent_dir(ctx.path());
    let staged = parent.join(name);
    let (current_id, current_generation): (String, i64) = recovery_guard
        .query_row("SELECT id,generation FROM catalogue LIMIT 1", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .map_err(CatalogError::from)?;
    if staged.exists() {
        if current_generation != record.local_generation {
            return Err(SyncError::Integrity("catalogue changed while a sync replacement was interrupted; local work is preserved and the staged snapshot was not installed".into()));
        }
        let metadata = std::fs::symlink_metadata(&staged)?;
        if !metadata.file_type().is_file() {
            return Err(SyncError::Integrity(
                "staged snapshot is not a regular file".into(),
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if metadata.nlink() > 1 {
                return Err(SyncError::Integrity(
                    "staged snapshot has multiple hard links".into(),
                ));
            }
        }
        if util::sha256_file(&staged)? != record.expected_digest {
            return Err(SyncError::Integrity(
                "staged snapshot digest does not match the durable recovery record".into(),
            ));
        }
        let backup = record
            .backup_path
            .as_ref()
            .ok_or_else(|| SyncError::Integrity("sync recovery backup path is missing".into()))?;
        if !std::fs::symlink_metadata(backup)?.file_type().is_file() {
            return Err(SyncError::Integrity(
                "sync recovery backup is not a regular file".into(),
            ));
        }
        util::verify_catalogue(backup, &current_id)?;
        util::verify_catalogue(&staged, &current_id)?;
        ctx.ensure_replaceable_path()?;
        ctx.replace_catalogue_file(&staged)?;
        util::sync_parent(ctx.path())?;
    } else {
        // A successful rename consumes the staged path. Distinguish that point
        // from a cleanup after failed rename by checking the target generation.
        let installed_generation = ctx
            .catalog()
            .replica_base()?
            .map(|(generation, _)| generation);
        if installed_generation != Some(record.server_generation)
            || util::sha256_file(ctx.path())? != record.expected_digest
        {
            crate::journal::set_phase(ctx, "retry")?;
            return Err(SyncError::Integrity("sync stopped before replacing the catalogue; your local file and backup are intact; run sync again".into()));
        }
        let backup = record
            .backup_path
            .as_ref()
            .ok_or_else(|| SyncError::Integrity("sync recovery backup path is missing".into()))?;
        if !std::fs::symlink_metadata(backup)?.file_type().is_file() {
            return Err(SyncError::Integrity(
                "sync recovery backup is not a regular file".into(),
            ));
        }
    }
    recovery_guard
        .execute_batch("ROLLBACK")
        .map_err(CatalogError::from)?;
    if crate::journal::has_unresolved(ctx)? {
        crate::journal::set_phase(ctx, "conflicts")?;
        ctx.set_sync_required(true)?;
    } else {
        crate::journal::clear_conflicts(ctx)?;
        ctx.set_sync_required(false)?;
    }
    Ok(true)
}

fn reserve_recovery_writer(path: &std::path::Path) -> Result<rusqlite::Connection> {
    let connection = rusqlite::Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(CatalogError::from)?;
    connection
        .busy_timeout(std::time::Duration::from_secs(5))
        .map_err(CatalogError::from)?;
    connection
        .execute_batch("BEGIN IMMEDIATE")
        .map_err(CatalogError::from)?;
    Ok(connection)
}

fn sync_locked(ctx: &mut Context, t: &dyn Transport, base: &str) -> Result<SyncReport> {
    let row = ctx
        .catalog()
        .catalogue()?
        .ok_or_else(|| SyncError::Protocol("local catalogue is not initialised".into()))?;
    ctx.report_progress(icatalog_engine::SyncProgress {
        phase: icatalog_engine::SyncPhase::Checking,
        completed: None,
        total: None,
    });
    let remote = info(t, base)?;
    if remote.id != row.id {
        return Err(SyncError::Protocol(
            "server catalogue id does not match the local catalogue".into(),
        ));
    }
    let baseline = ctx.catalog().replica_base()?;
    let (base_generation, changes) = delta::pending(&row.id, baseline.as_ref(), ctx.catalog())?;
    if baseline.is_some() && remote.generation < base_generation {
        return Err(SyncError::Protocol(
            "server generation moved backwards; refusing to replace the local catalogue".into(),
        ));
    }
    if baseline.is_some() && remote.generation == base_generation && changes.is_empty() {
        crate::journal::clear_conflicts(ctx)?;
        ctx.set_sync_required(false)?;
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
        ctx.report_progress(icatalog_engine::SyncProgress {
            phase: icatalog_engine::SyncPhase::Uploading,
            completed: None,
            total: None,
        });
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
    ctx.report_progress(icatalog_engine::SyncProgress {
        phase: icatalog_engine::SyncPhase::Downloading,
        completed: None,
        total: None,
    });
    let download = crate::download::prepare_with_progress(
        t,
        &join(base, PATH_CATALOGUE),
        ctx.path(),
        &row.id,
        minimum_generation,
        &mut |completed, total| {
            ctx.report_progress(icatalog_engine::SyncProgress {
                phase: icatalog_engine::SyncPhase::Downloading,
                completed: Some(completed),
                total,
            })
        },
    )?;
    ctx.report_progress(icatalog_engine::SyncProgress {
        phase: icatalog_engine::SyncPhase::Validating,
        completed: Some(download.bytes),
        total: Some(download.bytes),
    });
    let expected_digest = util::sha256_file(download.temp.path())?;
    let server_current: BTreeMap<_, _> = {
        let downloaded = icatalog_catalog::Catalog::open_read_only(download.temp.path())?;
        delta::current(&downloaded)?
            .into_iter()
            .map(|v| (v.key(), v))
            .collect()
    };
    // Windows cannot rename the staged source while its SQLite handle is open.
    // Only owned values escape the scope above; keep the local writer reservation
    // unchanged until Context closes it immediately before atomic replacement.
    let local_changes: BTreeMap<_, _> = changes.into_iter().map(|v| (v.key(), v)).collect();
    let mut durable_conflicts = Vec::new();
    for key in &conflicts {
        let local = local_changes.get(key).ok_or_else(|| {
            SyncError::Integrity(
                "server returned a conflict for an edit that was not uploaded".into(),
            )
        })?;
        let server = server_current.get(key).ok_or_else(|| {
            SyncError::Integrity(
                "conflicting server entity is absent from the downloaded catalogue".into(),
            )
        })?;
        durable_conflicts.push((key.clone(), local.clone(), server.clone()));
    }
    // No writes were made to the locked local catalogue. Its committed main
    // file is a consistent backup in DELETE journal mode.
    ctx.report_progress(icatalog_engine::SyncProgress {
        phase: icatalog_engine::SyncPhase::BackingUp,
        completed: None,
        total: None,
    });
    let backup = util::backup_local(ctx.path())?;
    if backup.is_none() {
        return Err(SyncError::Integrity(
            "local catalogue disappeared before its recovery backup was created".into(),
        ));
    }
    let staged_name = download
        .temp
        .path()
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(|| SyncError::Protocol("cannot safely journal staged download path".into()))?;
    // Latch the replica before making the recovery record visible. A crash in
    // either write leaves the desktop read-only until an explicit safe retry.
    ctx.set_sync_required(true)?;
    crate::journal::replace_conflicts(
        ctx,
        crate::journal::SnapshotRecovery {
            catalogue_id: &row.id,
            conflicts: &durable_conflicts,
            staged_name: Some(staged_name),
            backup_path: backup.as_deref(),
            local_generation: row.generation,
            server_generation: download.generation,
            expected_digest: &expected_digest,
        },
    )?;
    ctx.report_progress(icatalog_engine::SyncProgress {
        phase: icatalog_engine::SyncPhase::Replacing,
        completed: None,
        total: None,
    });
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
    if !conflicts.is_empty() {
        crate::journal::set_phase(ctx, "conflicts")?;
        ctx.set_sync_required(true)?;
    } else {
        crate::journal::clear_conflicts(ctx)?;
        ctx.set_sync_required(false)?;
    }
    ctx.report_progress(icatalog_engine::SyncProgress {
        phase: icatalog_engine::SyncPhase::Complete,
        completed: Some(download.bytes),
        total: Some(download.bytes),
    });
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
    ctx.ensure_replaceable_path()?;
    if !icatalog_catalog::valid_preview_hash(hash) {
        return Err(SyncError::Protocol("invalid preview hash".into()));
    }
    if icatalog_engine::previews::read_recoverable(ctx.catalog(), ctx.path(), hash)?.is_some() {
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod recovery_reservation_tests {
    use super::reserve_recovery_writer;
    use std::time::Duration;

    #[test]
    fn recovery_writer_reservation_blocks_a_concurrent_sqlite_writer() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("reservation.sqlite");
        drop(rusqlite::Connection::open(&path).unwrap());

        let reservation = reserve_recovery_writer(&path).unwrap();
        let competing_path = path.clone();
        let competing = std::thread::spawn(move || {
            let connection = rusqlite::Connection::open(competing_path).unwrap();
            connection.busy_timeout(Duration::from_millis(25)).unwrap();
            connection.execute_batch("BEGIN IMMEDIATE")
        });
        let error = competing.join().unwrap().unwrap_err();
        assert!(matches!(error, rusqlite::Error::SqliteFailure(ref code, _)
            if code.code == rusqlite::ErrorCode::DatabaseBusy
                || code.code == rusqlite::ErrorCode::DatabaseLocked));
        drop(reservation);

        let connection = rusqlite::Connection::open(path).unwrap();
        connection.execute_batch("BEGIN IMMEDIATE").unwrap();
        connection.execute_batch("ROLLBACK").unwrap();
    }
}
