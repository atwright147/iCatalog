//! Durable conflict drafts and interrupted-sync markers, kept beside the
//! replaceable catalogue so snapshot swaps cannot discard recovery state.
use crate::{delta, Result, SyncError};
use icatalog_engine::Context;
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const MAX_RECORD: usize = 64 * 1024 * 1024;
const MAX_RECOVERY_PATH_BYTES: usize = 32 * 1024;
const MAX_STAGED_NAME_BYTES: usize = 1024;
const MAX_RECOVERY_PHASE_BYTES: usize = 32;
const MAX_SESSION_ID_BYTES: usize = 128;
const MAX_CONFLICT_KEY_BYTES: usize = 4096;

fn path(ctx: &Context) -> Result<PathBuf> {
    ctx.sync_state_path().ok_or_else(|| {
        SyncError::Protocol("durable sync recovery requires an on-disk catalogue".into())
    })
}

fn companion_exists(ctx: &Context) -> Result<bool> {
    let path = path(ctx)?;
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
        Ok(metadata) if metadata.file_type().is_file() => Ok(true),
        Ok(_) => Err(SyncError::Integrity(
            "sync recovery companion is not a regular file".into(),
        )),
    }
}

pub(crate) fn open(ctx: &Context) -> Result<icatalog_catalog::sync_state::SyncStateDb> {
    let row = ctx
        .catalog()
        .catalogue()?
        .ok_or_else(|| SyncError::Protocol("local catalogue is not initialised".into()))?;
    let state = icatalog_catalog::sync_state::SyncStateDb::open(ctx.path(), &row.id, true)?;
    state.connection().execute_batch(
        "CREATE TABLE IF NOT EXISTS state(key TEXT PRIMARY KEY, value INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS sync_conflict(
           key TEXT PRIMARY KEY, local_record BLOB NOT NULL, server_record BLOB NOT NULL
         );
         CREATE TABLE IF NOT EXISTS sync_choice(
           key TEXT NOT NULL REFERENCES sync_conflict(key) ON DELETE CASCADE,
           field TEXT NOT NULL, choice TEXT NOT NULL,
           PRIMARY KEY(key,field)
         );
         CREATE TABLE IF NOT EXISTS sync_recovery(
           singleton INTEGER PRIMARY KEY CHECK(singleton=1), phase TEXT NOT NULL,
           backup_path BLOB, staged_path TEXT, local_generation INTEGER, server_generation INTEGER,
           expected_digest TEXT
         );
         CREATE TABLE IF NOT EXISTS sync_resolution(
           key TEXT PRIMARY KEY REFERENCES sync_conflict(key), merged_record BLOB NOT NULL
         );
         CREATE TABLE IF NOT EXISTS sync_session(
           singleton INTEGER PRIMARY KEY CHECK(singleton=1), session_id TEXT NOT NULL
         );",
    )?;
    Ok(state)
}

fn open_read(ctx: &Context) -> Result<Connection> {
    let row = ctx
        .catalog()
        .catalogue()?
        .ok_or_else(|| SyncError::Protocol("local catalogue is not initialised".into()))?;
    Ok(icatalog_catalog::sync_state::SyncStateDb::open(
        ctx.path(),
        &row.id,
        false,
    )?)
    .map(|state| state.into_connection())
}

fn has_table(conn: &Connection, table: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        [table],
        |r| r.get(0),
    )?)
}

#[derive(Debug, Clone)]
pub(crate) struct StoredConflict {
    pub key: String,
    pub local: delta::Change,
    pub server: delta::Change,
    pub choices: Vec<(String, String)>,
}

fn pack(id: &str, value: &delta::Change) -> Result<Vec<u8>> {
    let bytes = delta::encode(id, std::slice::from_ref(value))?;
    if bytes.len() > MAX_RECORD {
        return Err(SyncError::Protocol(
            "conflict draft exceeds sync limit".into(),
        ));
    }
    Ok(bytes)
}

fn unpack(bytes: &[u8]) -> Result<delta::Change> {
    if bytes.len() > MAX_RECORD {
        return Err(SyncError::Integrity(
            "stored conflict draft exceeds sync limit".into(),
        ));
    }
    let (_, mut changes) = delta::decode(bytes)?;
    if changes.len() != 1 {
        return Err(SyncError::Integrity("invalid stored conflict draft".into()));
    }
    changes
        .pop()
        .ok_or_else(|| SyncError::Integrity("missing stored conflict draft".into()))
}

pub(crate) struct SnapshotRecovery<'a> {
    pub catalogue_id: &'a str,
    pub conflicts: &'a [(String, delta::Change, delta::Change)],
    pub staged_name: Option<&'a str>,
    pub backup_path: Option<&'a Path>,
    pub local_generation: i64,
    pub server_generation: i64,
    pub expected_digest: &'a str,
}

pub(crate) fn replace_conflicts(ctx: &Context, snapshot: SnapshotRecovery<'_>) -> Result<()> {
    if snapshot
        .staged_name
        .is_some_and(|name| name.len() > MAX_STAGED_NAME_BYTES)
        || snapshot.expected_digest.len() != 64
        || !snapshot
            .expected_digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(SyncError::Protocol(
            "invalid staged snapshot recovery metadata".into(),
        ));
    }
    let mut state = open(ctx)?;
    let tx = state.connection_mut().transaction()?;
    tx.execute("DELETE FROM sync_resolution", [])?;
    tx.execute("DELETE FROM sync_choice", [])?;
    tx.execute("DELETE FROM sync_conflict", [])?;
    let mut stored_bytes = 0usize;
    for (key, local, server) in snapshot.conflicts {
        if key.len() > MAX_CONFLICT_KEY_BYTES || key != &local.key() || key != &server.key() {
            return Err(SyncError::Protocol(
                "invalid or oversized conflict recovery key".into(),
            ));
        }
        let local = pack(snapshot.catalogue_id, local)?;
        let server = pack(snapshot.catalogue_id, server)?;
        stored_bytes = stored_bytes
            .checked_add(local.len())
            .and_then(|n| n.checked_add(server.len()))
            .ok_or_else(|| {
                SyncError::Protocol("conflict recovery data exceeds the sync limit".into())
            })?;
        if stored_bytes > icatalog_catalog::MAX_SYNC_BYTES {
            return Err(SyncError::Protocol(
                "conflict recovery data exceeds the sync limit".into(),
            ));
        }
        tx.execute(
            "INSERT INTO sync_conflict(key,local_record,server_record) VALUES(?1,?2,?3)",
            params![key, local, server],
        )?;
    }
    let backup_bytes = snapshot.backup_path.map(encode_path).transpose()?;
    tx.execute("INSERT INTO sync_recovery(singleton,phase,backup_path,staged_path,local_generation,server_generation,expected_digest) VALUES(1,'snapshot_ready',?1,?2,?3,?4,?5) ON CONFLICT(singleton) DO UPDATE SET phase=excluded.phase,backup_path=excluded.backup_path,staged_path=excluded.staged_path,local_generation=excluded.local_generation,server_generation=excluded.server_generation,expected_digest=excluded.expected_digest", params![backup_bytes,snapshot.staged_name,snapshot.local_generation,snapshot.server_generation,snapshot.expected_digest])?;
    if snapshot.conflicts.is_empty() {
        tx.execute("DELETE FROM sync_session", [])?;
    } else {
        tx.execute(
            "INSERT INTO sync_session(singleton,session_id) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET session_id=excluded.session_id",
            [new_session_id()],
        )?;
    }
    tx.commit()?;
    Ok(())
}

pub(crate) fn prepare_resolution(
    ctx: &Context,
    id: &str,
    records: &[(String, delta::Change)],
) -> Result<()> {
    let mut state = open(ctx)?;
    let tx = state.connection_mut().transaction()?;
    tx.execute("DELETE FROM sync_resolution", [])?;
    let mut stored_bytes = 0usize;
    for (key, record) in records {
        if key.len() > MAX_CONFLICT_KEY_BYTES || key != &record.key() {
            return Err(SyncError::Protocol(
                "invalid or oversized conflict resolution key".into(),
            ));
        }
        let record = pack(id, record)?;
        stored_bytes = stored_bytes.checked_add(record.len()).ok_or_else(|| {
            SyncError::Protocol("conflict resolution exceeds the sync limit".into())
        })?;
        if stored_bytes > icatalog_catalog::MAX_SYNC_BYTES {
            return Err(SyncError::Protocol(
                "conflict resolution exceeds the sync limit".into(),
            ));
        }
        tx.execute(
            "INSERT INTO sync_resolution(key,merged_record) VALUES(?1,?2)",
            params![key, record],
        )?;
    }
    tx.execute(
        "UPDATE sync_recovery SET phase='applying' WHERE singleton=1",
        [],
    )?;
    tx.commit()?;
    Ok(())
}

pub(crate) fn resolution_records(ctx: &Context) -> Result<Vec<(String, delta::Change)>> {
    let state = open_read(ctx)?;
    let mut stmt = state.prepare("SELECT CASE WHEN length(CAST(key AS BLOB))<=?1 THEN key END,CASE WHEN length(merged_record)<=?2 THEN merged_record END FROM sync_resolution ORDER BY key LIMIT ?3")?;
    let rows = stmt.query_map(
        params![
            MAX_CONFLICT_KEY_BYTES as i64,
            MAX_RECORD as i64,
            icatalog_catalog::MAX_SYNC_ROWS as i64 + 1
        ],
        |r| {
            Ok((
                r.get::<_, Option<String>>(0)?,
                r.get::<_, Option<Vec<u8>>>(1)?,
            ))
        },
    )?;
    let mut out = Vec::new();
    let mut stored_bytes = 0usize;
    for row in rows {
        let (key, bytes) = row?;
        let key = key.ok_or_else(|| {
            SyncError::Integrity("stored conflict resolution key exceeds its size limit".into())
        })?;
        let bytes = bytes.ok_or_else(|| {
            SyncError::Integrity("stored conflict resolution exceeds its size limit".into())
        })?;
        stored_bytes = stored_bytes.checked_add(bytes.len()).ok_or_else(|| {
            SyncError::Integrity("stored conflict resolution exceeds its size limit".into())
        })?;
        if out.len() >= icatalog_catalog::MAX_SYNC_ROWS
            || stored_bytes > icatalog_catalog::MAX_SYNC_BYTES
        {
            return Err(SyncError::Integrity(
                "stored conflict resolution exceeds its size limit".into(),
            ));
        }
        out.push((key, unpack(&bytes)?));
    }
    Ok(out)
}

pub(crate) fn set_phase(ctx: &Context, phase: &str) -> Result<()> {
    if phase.is_empty() || phase.len() > MAX_RECOVERY_PHASE_BYTES {
        return Err(SyncError::Protocol(
            "sync recovery phase exceeds its size limit".into(),
        ));
    }
    let mut state = open(ctx)?;
    state.connection_mut().execute(
        "UPDATE sync_recovery SET phase=?1 WHERE singleton=1",
        [phase],
    )?;
    Ok(())
}

pub(crate) fn conflicts(ctx: &Context) -> Result<Vec<StoredConflict>> {
    if !companion_exists(ctx)? {
        return Ok(Vec::new());
    }
    let state = open_read(ctx)?;
    let conn = &state;
    if !has_table(conn, "sync_conflict")? {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare("SELECT CASE WHEN length(CAST(key AS BLOB))<=?1 THEN key END,CASE WHEN length(local_record)<=?2 THEN local_record END,CASE WHEN length(server_record)<=?2 THEN server_record END FROM sync_conflict ORDER BY key LIMIT ?3")?;
    let rows = stmt.query_map(
        params![
            MAX_CONFLICT_KEY_BYTES as i64,
            MAX_RECORD as i64,
            icatalog_catalog::MAX_SYNC_ROWS as i64 + 1
        ],
        |r| {
            Ok((
                r.get::<_, Option<String>>(0)?,
                r.get::<_, Option<Vec<u8>>>(1)?,
                r.get::<_, Option<Vec<u8>>>(2)?,
            ))
        },
    )?;
    let mut out = Vec::new();
    let mut stored_bytes = 0usize;
    let mut count = 0usize;
    for row in rows {
        let (key, local, server) = row?;
        let key = key.ok_or_else(|| {
            SyncError::Integrity("stored conflict key exceeds its size limit".into())
        })?;
        let local = local.ok_or_else(|| {
            SyncError::Integrity("stored conflict draft exceeds its size limit".into())
        })?;
        let server = server.ok_or_else(|| {
            SyncError::Integrity("stored conflict draft exceeds its size limit".into())
        })?;
        count = count.saturating_add(1);
        stored_bytes = stored_bytes
            .checked_add(local.len())
            .and_then(|n| n.checked_add(server.len()))
            .ok_or_else(|| {
                SyncError::Integrity("stored conflict recovery data exceeds limits".into())
            })?;
        if count > icatalog_catalog::MAX_SYNC_ROWS
            || stored_bytes > icatalog_catalog::MAX_SYNC_BYTES
        {
            return Err(SyncError::Integrity(
                "stored conflict recovery data exceeds limits".into(),
            ));
        }
        let mut choices_stmt = conn.prepare("SELECT CASE WHEN length(CAST(field AS BLOB))<=256 THEN field END,CASE WHEN length(CAST(choice AS BLOB))<=16 THEN choice END FROM sync_choice WHERE key=?1 ORDER BY field LIMIT 33")?;
        let choices = choices_stmt
            .query_map([&key], |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, Option<String>>(1)?,
                ))
            })?
            .map(|result| {
                result
                    .map(|(field, choice)| (field.unwrap_or_default(), choice.unwrap_or_default()))
            })
            .collect::<std::result::Result<Vec<(String, String)>, _>>()?;
        if choices.len() > 32
            || choices
                .iter()
                .any(|(field, choice)| field.is_empty() || choice.is_empty())
        {
            return Err(SyncError::Integrity(
                "stored conflict choices exceed their size limits".into(),
            ));
        }
        out.push(StoredConflict {
            key,
            local: unpack(&local)?,
            server: unpack(&server)?,
            choices,
        });
    }
    Ok(out)
}

pub(crate) fn choose(ctx: &Context, key: &str, field: &str, choice: &str) -> Result<()> {
    if key.is_empty()
        || key.len() > MAX_CONFLICT_KEY_BYTES
        || field.is_empty()
        || field.len() > 256
        || choice.len() > 16
    {
        return Err(SyncError::Protocol(
            "conflict choice identifier exceeds its size limit".into(),
        ));
    }
    let mut state = open(ctx)?;
    let conn = state.connection_mut();
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sync_conflict WHERE key=?1)",
        [key],
        |r| r.get(0),
    )?;
    if !exists {
        return Err(SyncError::Protocol(
            "conflict key is not in the recovery journal".into(),
        ));
    }
    conn.execute("INSERT INTO sync_choice(key,field,choice) VALUES(?1,?2,?3) ON CONFLICT(key,field) DO UPDATE SET choice=excluded.choice", params![key,field,choice])?;
    Ok(())
}

pub(crate) fn clear_conflicts(ctx: &Context) -> Result<()> {
    let mut state = open(ctx)?;
    let conn = state.connection_mut();
    conn.execute("DELETE FROM sync_resolution", [])?;
    conn.execute("DELETE FROM sync_choice", [])?;
    conn.execute("DELETE FROM sync_conflict", [])?;
    conn.execute("DELETE FROM sync_recovery", [])?;
    conn.execute("DELETE FROM sync_session", [])?;
    Ok(())
}

pub(crate) fn session_id(ctx: &Context) -> Result<Option<String>> {
    let conflicts = conflicts(ctx)?;
    if conflicts.is_empty() {
        return Ok(None);
    }
    let conn = open_read(ctx)?;
    let existing = if has_table(&conn, "sync_session")? {
        let row = conn.query_row(
            "SELECT length(CAST(session_id AS BLOB)),CASE WHEN length(CAST(session_id AS BLOB))<=?1 THEN session_id END FROM sync_session WHERE singleton=1",
            [MAX_SESSION_ID_BYTES as i64],
            |row| Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, Option<String>>(1)?)),
        )
        .optional()?;
        match row {
            None => None,
            Some((Some(length), Some(session))) if length <= MAX_SESSION_ID_BYTES as i64 => {
                Some(session)
            }
            Some((Some(length), _)) if length > MAX_SESSION_ID_BYTES as i64 => {
                return Err(SyncError::Integrity(
                    "stored conflict session identifier exceeds its size limit".into(),
                ));
            }
            Some(_) => {
                return Err(SyncError::Integrity(
                    "stored conflict session identifier is missing or invalid".into(),
                ));
            }
        }
    } else {
        None
    };
    if let Some(session) = existing {
        if session.is_empty() || session.len() > MAX_SESSION_ID_BYTES {
            return Err(SyncError::Integrity(
                "stored conflict session identifier is invalid".into(),
            ));
        }
        return Ok(Some(session));
    }
    // Older companion journals predate explicit session IDs. Derive a stable,
    // read-only identity from their immutable local/server conflict payloads.
    let row = ctx
        .catalog()
        .catalogue()?
        .ok_or_else(|| SyncError::Integrity("active catalogue is not initialised".into()))?;
    let mut digest = Sha256::new();
    digest.update(b"icatalog-legacy-conflict-session-v1");
    digest.update(row.id.as_bytes());
    for conflict in conflicts {
        digest.update(conflict.key.as_bytes());
        digest.update(pack(&row.id, &conflict.local)?);
        digest.update(pack(&row.id, &conflict.server)?);
    }
    let digest = digest.finalize();
    let session = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(Some(format!("legacy-{session}")))
}

pub(crate) fn validate_session(ctx: &Context, supplied: &str) -> Result<()> {
    if supplied.len() > MAX_SESSION_ID_BYTES || supplied.is_empty() {
        return Err(SyncError::Protocol(
            "invalid conflict session identifier".into(),
        ));
    }
    let expected = session_id(ctx)?.ok_or_else(|| {
        SyncError::Protocol("there is no active conflict session; refresh the conflict list".into())
    })?;
    if supplied != expected {
        return Err(SyncError::Protocol(
            "conflict session changed; refresh the conflict list before continuing".into(),
        ));
    }
    Ok(())
}

fn new_session_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let seq = NEXT.fetch_add(1, Ordering::Relaxed);
    format!("{:032x}-{:x}-{:x}", time, std::process::id(), seq)
}

pub(crate) fn has_unresolved(ctx: &Context) -> Result<bool> {
    Ok(!conflicts(ctx)?.is_empty())
}

pub(crate) fn recovery_phase(ctx: &Context) -> Result<Option<String>> {
    if !companion_exists(ctx)? {
        return Ok(None);
    }
    let conn = open_read(ctx)?;
    if !has_table(&conn, "sync_recovery")? {
        return Ok(None);
    }
    let row = conn
        .query_row(
            "SELECT length(CAST(phase AS BLOB)),CASE WHEN length(CAST(phase AS BLOB))<=?1 THEN phase END FROM sync_recovery WHERE singleton=1",
            [MAX_RECOVERY_PHASE_BYTES as i64],
            |r| Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, Option<String>>(1)?)),
        )
        .optional()?;
    match row {
        None => Ok(None),
        Some((Some(length), _)) if length > MAX_RECOVERY_PHASE_BYTES as i64 => Err(
            SyncError::Integrity("sync recovery phase exceeds its size limit".into()),
        ),
        Some((_, Some(phase))) => Ok(Some(phase)),
        Some(_) => Err(SyncError::Integrity(
            "sync recovery phase is missing or invalid".into(),
        )),
    }
}

pub(crate) struct RecoveryRecord {
    pub staged_name: String,
    pub backup_path: Option<PathBuf>,
    pub local_generation: i64,
    pub server_generation: i64,
    pub expected_digest: String,
}

pub(crate) fn staged_snapshot(ctx: &Context) -> Result<Option<RecoveryRecord>> {
    if !companion_exists(ctx)? {
        return Ok(None);
    }
    let conn = open_read(ctx)?;
    if !has_table(&conn, "sync_recovery")? {
        return Ok(None);
    }
    let phase = conn
        .query_row(
            "SELECT length(CAST(phase AS BLOB)),CASE WHEN length(CAST(phase AS BLOB))<=?1 THEN phase END FROM sync_recovery WHERE singleton=1",
            [MAX_RECOVERY_PHASE_BYTES as i64],
            |r| Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, Option<String>>(1)?)),
        )
        .optional()?;
    let Some((phase_len, phase)) = phase else {
        return Ok(None);
    };
    if phase_len.is_some_and(|length| length > MAX_RECOVERY_PHASE_BYTES as i64) {
        return Err(SyncError::Integrity(
            "sync recovery phase exceeds its size limit".into(),
        ));
    }
    let phase = phase
        .ok_or_else(|| SyncError::Integrity("sync recovery phase is missing or invalid".into()))?;
    if phase != "snapshot_ready" {
        return Ok(None);
    }
    let row = conn
        .query_row(
            "SELECT length(CAST(staged_path AS BLOB)),CASE WHEN length(CAST(staged_path AS BLOB))<=?1 THEN staged_path END,length(backup_path),CASE WHEN length(backup_path)<=?2 THEN backup_path END,local_generation,server_generation,length(CAST(expected_digest AS BLOB)),CASE WHEN length(CAST(expected_digest AS BLOB))<=?3 THEN expected_digest END FROM sync_recovery WHERE singleton=1 AND phase='snapshot_ready'",
            params![
                MAX_STAGED_NAME_BYTES as i64,
                MAX_RECOVERY_PATH_BYTES as i64,
                128_i64
            ],
            |r| {
                Ok((
                    r.get::<_, Option<i64>>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                    r.get::<_, Option<Vec<u8>>>(3)?,
                    r.get::<_, Option<i64>>(4)?,
                    r.get::<_, Option<i64>>(5)?,
                    r.get::<_, Option<i64>>(6)?,
                    r.get::<_, Option<String>>(7)?,
                ))
            },
        )
        .optional()?;
    let Some((name_len, name, backup_len, backup, local, server, digest_len, digest)) = row else {
        return Ok(None);
    };
    if name_len.is_some_and(|length| length > MAX_STAGED_NAME_BYTES as i64) {
        return Err(SyncError::Integrity(
            "staged snapshot name in recovery state exceeds its size limit".into(),
        ));
    }
    if backup_len.is_some_and(|length| length > MAX_RECOVERY_PATH_BYTES as i64) {
        return Err(SyncError::Integrity(
            "backup path in recovery state exceeds its size limit".into(),
        ));
    }
    if digest_len.is_some_and(|length| length > 128) {
        return Err(SyncError::Integrity(
            "snapshot digest in recovery state exceeds its size limit".into(),
        ));
    }
    let staged_name =
        name.ok_or_else(|| SyncError::Integrity("missing staged snapshot name".into()))?;
    let expected_digest =
        digest.ok_or_else(|| SyncError::Integrity("missing expected snapshot digest".into()))?;
    if expected_digest.len() != 64 || !expected_digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(SyncError::Integrity(
            "snapshot digest in recovery state is invalid".into(),
        ));
    }
    Ok(Some(RecoveryRecord {
        staged_name,
        backup_path: backup.map(|bytes| decode_path(&bytes)).transpose()?,
        local_generation: local.ok_or_else(|| {
            SyncError::Integrity("missing local generation in recovery state".into())
        })?,
        server_generation: server.ok_or_else(|| {
            SyncError::Integrity("missing server generation in recovery state".into())
        })?,
        expected_digest,
    }))
}

pub(crate) fn recovery_backup(ctx: &Context) -> Result<Option<PathBuf>> {
    if !companion_exists(ctx)? {
        return Ok(None);
    }
    let conn = open_read(ctx)?;
    if !has_table(&conn, "sync_recovery")? {
        return Ok(None);
    }
    let row = conn
        .query_row(
            "SELECT length(backup_path),CASE WHEN length(backup_path)<=?1 THEN backup_path END FROM sync_recovery WHERE singleton=1",
            [MAX_RECOVERY_PATH_BYTES as i64],
            |r| Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, Option<Vec<u8>>>(1)?)),
        )
        .optional()?;
    let Some((length, bytes)) = row else {
        return Ok(None);
    };
    if length.is_some_and(|size| size > MAX_RECOVERY_PATH_BYTES as i64) {
        return Err(SyncError::Integrity(
            "backup path in recovery state exceeds its size limit".into(),
        ));
    }
    let Some(backup) = bytes.map(|b| decode_path(&b)).transpose()? else {
        return Ok(None);
    };
    let metadata = std::fs::symlink_metadata(&backup).map_err(|error| {
        SyncError::Integrity(format!(
            "sync recovery backup is unavailable at {}: {error}; restore the backup or retry sync",
            backup.display()
        ))
    })?;
    if !metadata.file_type().is_file() {
        return Err(SyncError::Integrity(
            "sync recovery backup is not a regular file; restore the backup or retry sync".into(),
        ));
    }
    let backup_catalogue = icatalog_catalog::Catalog::open_read_only(&backup).map_err(|error| {
        SyncError::Integrity(format!(
            "sync recovery backup is unreadable at {}: {error}; restore the backup or retry sync",
            backup.display()
        ))
    })?;
    let integrity: String = backup_catalogue
        .connection()
        .query_row("PRAGMA quick_check(1)", [], |row| row.get(0))
        .map_err(|error| {
            SyncError::Integrity(format!(
                "sync recovery backup failed its integrity check at {}: {error}; preserve it and restore a valid backup",
                backup.display()
            ))
        })?;
    if integrity != "ok" {
        return Err(SyncError::Integrity(format!(
            "sync recovery backup is corrupt at {}: {integrity}; preserve it and restore a valid backup",
            backup.display()
        )));
    }
    let expected_id = ctx
        .catalog()
        .catalogue()?
        .ok_or_else(|| SyncError::Integrity("active catalogue is not initialised".into()))?
        .id;
    let actual_id = backup_catalogue
        .catalogue()?
        .ok_or_else(|| SyncError::Integrity("sync recovery backup is not initialised".into()))?
        .id;
    if actual_id != expected_id {
        return Err(SyncError::Integrity(
            "sync recovery backup belongs to a different catalogue; preserve it and restore the correct backup".into(),
        ));
    }
    Ok(Some(backup))
}

fn encode_path(path: &Path) -> Result<Vec<u8>> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        if path.as_os_str().as_bytes().len() >= MAX_RECOVERY_PATH_BYTES {
            return Err(SyncError::Protocol(
                "sync recovery path exceeds its size limit".into(),
            ));
        }
        let mut bytes = vec![b'u'];
        bytes.extend_from_slice(path.as_os_str().as_bytes());
        Ok(bytes)
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        if path.as_os_str().encode_wide().count() > (MAX_RECOVERY_PATH_BYTES - 1) / 2 {
            return Err(SyncError::Protocol(
                "sync recovery path exceeds its size limit".into(),
            ));
        }
        let mut bytes = vec![b'w'];
        for word in path.as_os_str().encode_wide() {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        Ok(bytes)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        Err(SyncError::Protocol(
            "unsupported platform path encoding".into(),
        ))
    }
}

fn decode_path(bytes: &[u8]) -> Result<PathBuf> {
    if bytes.len() > MAX_RECOVERY_PATH_BYTES {
        return Err(SyncError::Integrity(
            "sync recovery path exceeds its size limit".into(),
        ));
    }
    let Some((kind, body)) = bytes.split_first() else {
        return Err(SyncError::Integrity(
            "invalid recovery path encoding".into(),
        ));
    };
    match *kind {
        #[cfg(unix)]
        b'u' => {
            use std::os::unix::ffi::OsStringExt;
            Ok(PathBuf::from(std::ffi::OsString::from_vec(body.to_vec())))
        }
        #[cfg(windows)]
        b'w' => {
            use std::os::windows::ffi::OsStringExt;
            if body.len() % 2 != 0 {
                return Err(SyncError::Integrity(
                    "invalid recovery path encoding".into(),
                ));
            }
            let words = body
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect::<Vec<_>>();
            Ok(PathBuf::from(std::ffi::OsString::from_wide(&words)))
        }
        _ => Err(SyncError::Integrity(
            "recovery path belongs to another platform".into(),
        )),
    }
}
