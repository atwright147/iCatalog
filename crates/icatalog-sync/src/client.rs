//! The sync client: decide pull or push from the generation, verify downloads,
//! swap atomically and keep a timestamped backup before any overwrite.
//! Implements the conflict policy in `docs/sync.md` sections 4-6.

use std::path::{Path, PathBuf};

use icatalog_engine::Context;

use crate::error::{Result, SyncError};
use crate::protocol::{CatalogueInfo, HEADER_GENERATION, HEADER_ID, PATH_CATALOGUE, PATH_INFO};
use crate::transport::{HttpResponse, Transport};
use crate::util;

/// What a sync run did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncAction {
    /// Local and server were level and the local copy was unchanged since the
    /// last sync: nothing was sent and the server generation did not move.
    UpToDate,
    /// Local and server were level: the local copy was published.
    Pushed,
    /// The server was ahead: the local copy was replaced (backed up first).
    Pulled { backup: Option<PathBuf> },
    /// A push was refused (the generation moved): the local copy was backed up
    /// and resynced from the server.
    Resynced { backup: Option<PathBuf> },
}

/// The outcome of one sync run.
#[derive(Debug, Clone)]
pub struct SyncReport {
    pub action: SyncAction,
    pub local_generation: i64,
    pub server_generation: i64,
    pub bytes: u64,
}

/// `GET /info`: the server's catalogue id and generation.
pub fn info(transport: &dyn Transport, base: &str) -> Result<CatalogueInfo> {
    let resp = transport.get(&join(base, PATH_INFO))?;
    if resp.status != 200 {
        return Err(SyncError::Http(format!(
            "GET /info returned {}",
            resp.status
        )));
    }
    let body = std::str::from_utf8(&resp.body)
        .map_err(|_| SyncError::Protocol("info body is not UTF-8".into()))?;
    CatalogueInfo::parse(body)
}

/// Download the server snapshot to a verified temp file next to `local`,
/// returning the temp path, the server info and the byte count. The caller
/// performs the atomic swap so a failure leaves the local copy untouched.
pub fn download(
    transport: &dyn Transport,
    base: &str,
    local: &Path,
    expect_id: &str,
) -> Result<(PathBuf, CatalogueInfo, u64)> {
    let resp = transport.get(&join(base, PATH_CATALOGUE))?;
    if resp.status != 200 {
        return Err(SyncError::Http(format!(
            "GET /catalogue returned {}",
            resp.status
        )));
    }
    let id = resp
        .header(HEADER_ID)
        .ok_or_else(|| SyncError::Protocol("snapshot is missing X-Catalogue-Id".into()))?;
    if id != expect_id {
        return Err(SyncError::Protocol(format!(
            "server catalogue id '{id}' does not match local '{expect_id}'"
        )));
    }
    let generation = header_generation(&resp)?;
    let temp = util::temp_path(&util::parent_dir(local), "download");
    std::fs::write(&temp, &resp.body)?;
    if let Err(e) = util::verify_catalogue(&temp, expect_id) {
        let _ = std::fs::remove_file(&temp);
        return Err(e);
    }
    Ok((
        temp,
        CatalogueInfo {
            id: id.to_string(),
            generation,
        },
        resp.body.len() as u64,
    ))
}

/// The result of pushing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushOutcome {
    /// Accepted; the server generation is now this value.
    Accepted { generation: i64 },
    /// Refused; the server is at this generation.
    Refused { generation: i64 },
}

/// `POST /catalogue?expected=<base>` with the local snapshot.
pub fn push(
    transport: &dyn Transport,
    base: &str,
    expected: i64,
    body: &[u8],
) -> Result<PushOutcome> {
    let resp = transport.post(
        &format!("{}?expected={expected}", join(base, PATH_CATALOGUE)),
        body,
    )?;
    match resp.status {
        200 => Ok(PushOutcome::Accepted {
            generation: header_generation(&resp)?,
        }),
        409 => Ok(PushOutcome::Refused {
            generation: header_generation(&resp)?,
        }),
        other => Err(SyncError::Http(format!("POST /catalogue returned {other}"))),
    }
}

/// One sync run against `base`, per `docs/sync.md` sections 4-6.
pub fn sync_once(ctx: &mut Context, transport: &dyn Transport, base: &str) -> Result<SyncReport> {
    let row = ctx
        .catalog()
        .catalogue()?
        .ok_or_else(|| SyncError::Protocol("local catalogue is not initialised".into()))?;
    let local = ctx.path().to_path_buf();

    let info = info(transport, base)?;
    if info.id != row.id {
        return Err(SyncError::Protocol(format!(
            "server catalogue id '{}' does not match local '{}'",
            info.id, row.id
        )));
    }

    use std::cmp::Ordering;
    match info.generation.cmp(&row.generation) {
        // Server ahead: replace the local copy (backed up first).
        Ordering::Greater => {
            let backup = util::backup_local(&local)?;
            pull(
                ctx,
                transport,
                base,
                &row.id,
                SyncAction::Pulled { backup },
                row.generation,
            )
        }
        // Level: publish local state unless nothing changed since the last
        // sync (then a push would only bump the generation for no reason).
        Ordering::Equal => {
            let bytes = std::fs::read(&local)?;
            let fingerprint = util::fingerprint(&bytes);
            if util::read_state(&local).as_deref() == Some(fingerprint.as_str()) {
                return Ok(SyncReport {
                    action: SyncAction::UpToDate,
                    local_generation: row.generation,
                    server_generation: info.generation,
                    bytes: 0,
                });
            }
            match push(transport, base, row.generation, &bytes)? {
                PushOutcome::Accepted { generation } => {
                    ctx.catalog().set_generation(generation)?;
                    record_state(&local);
                    Ok(SyncReport {
                        action: SyncAction::Pushed,
                        local_generation: row.generation,
                        server_generation: generation,
                        bytes: bytes.len() as u64,
                    })
                }
                PushOutcome::Refused { .. } => {
                    let backup = util::backup_local(&local)?;
                    pull(
                        ctx,
                        transport,
                        base,
                        &row.id,
                        SyncAction::Resynced { backup },
                        row.generation,
                    )
                }
            }
        }
        // Should never happen with a well-behaved server; never overwrite it.
        Ordering::Less => Err(SyncError::Protocol(format!(
            "local generation {} is ahead of the server ({}); refusing to overwrite",
            row.generation, info.generation
        ))),
    }
}

/// Download, verify and atomically swap in a server snapshot.
fn pull(
    ctx: &mut Context,
    transport: &dyn Transport,
    base: &str,
    expect_id: &str,
    action: SyncAction,
    local_generation: i64,
) -> Result<SyncReport> {
    let local = ctx.path().to_path_buf();
    let (temp, info, bytes) = download(transport, base, &local, expect_id)?;
    if let Err(e) = ctx.replace_catalogue_file(&temp) {
        let _ = std::fs::remove_file(&temp);
        return Err(e.into());
    }
    // We are now in sync with the server: remember this exact state.
    record_state(&local);
    Ok(SyncReport {
        action,
        local_generation,
        server_generation: info.generation,
        bytes,
    })
}

/// Record the current local file's fingerprint in its sync marker, so an
/// unchanged copy can skip the next push. Best effort (see `util::write_state`).
fn record_state(local: &Path) {
    if let Ok(bytes) = std::fs::read(local) {
        util::write_state(local, &util::fingerprint(&bytes));
    }
}

fn header_generation(resp: &HttpResponse) -> Result<i64> {
    resp.header(HEADER_GENERATION)
        .ok_or_else(|| SyncError::Protocol("response is missing X-Generation".into()))?
        .trim()
        .parse::<i64>()
        .map_err(|_| SyncError::Protocol("bad X-Generation header".into()))
}

/// Join a base URL and a protocol path, tolerating a trailing slash.
fn join(base: &str, path: &str) -> String {
    format!("{}{}", base.trim_end_matches('/'), path)
}
