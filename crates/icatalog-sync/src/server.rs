//! Server-authoritative catalogue, revision-checked user edits and pull-only previews.
use crate::{
    delta::{self, Change, Value},
    protocol::{self, HEADER_GENERATION, HEADER_ID, PATH_CATALOGUE, PATH_INFO},
    util, Result, SyncError,
};
use icatalog_catalog::{CatalogError, MAX_SYNC_BYTES};
use icatalog_engine::Context;
use std::collections::BTreeMap;
use std::io::Read;
use tiny_http::{Header, Request, Response, Server};

pub fn bind(addr: &str) -> Result<Server> {
    Server::http(addr).map_err(|e| SyncError::Http(format!("cannot bind {addr}: {e}")))
}
pub fn serve_loop(server: Server, ctx: &mut Context) -> Result<()> {
    if ctx.catalog().is_replica()? {
        return Err(SyncError::Protocol(
            "a synced replica cannot serve as the authoritative catalogue".into(),
        ));
    }
    loop {
        let request = server.recv().map_err(SyncError::Io)?;
        if let Err(e) = handle(request, ctx) {
            eprintln!("{} serve: {e}", icatalog_catalog::APP_NAME);
        }
    }
}
fn handle(request: Request, ctx: &mut Context) -> Result<()> {
    let method = request.method().as_str().to_owned();
    let url = request.url().to_owned();
    let (path, _) = protocol::split_url(&url);
    match (method.as_str(), path) {
        ("GET", PATH_INFO) => {
            let row = ctx
                .catalog()
                .catalogue()?
                .ok_or_else(|| SyncError::Protocol("server catalogue is not initialised".into()))?;
            respond_text(
                request,
                200,
                &protocol::CatalogueInfo {
                    id: row.id,
                    generation: row.generation,
                }
                .encode(),
            )
        }
        ("GET", PATH_CATALOGUE) => respond_snapshot(request, ctx),
        ("POST", "/annotations") => handle_edits(request, ctx),
        ("POST", PATH_CATALOGUE) => respond_text(
            request,
            405,
            "snapshot uploads are retired; use protocol v2 user edits",
        ),
        ("GET", p) if p.starts_with("/preview/") => {
            let hash = p.strip_prefix("/preview/").unwrap_or("");
            if !icatalog_catalog::valid_preview_hash(hash) {
                return respond_text(request, 400, "invalid preview hash");
            }
            match icatalog_engine::previews::read(ctx.catalog(), ctx.path(), hash) {
                Ok(Some(bytes)) => respond_bytes(request, 200, bytes, None, "image/jpeg"),
                Ok(None) => respond_text(request, 404, "preview unavailable"),
                Err(e) => respond_text(request, 500, &format!("preview unavailable: {e}")),
            }
        }
        _ => respond_text(request, 404, "not found"),
    }
}
fn respond_snapshot(request: Request, ctx: &Context) -> Result<()> {
    let temp = util::TempPath::reserve(ctx.path(), "snapshot")?;
    let path = temp
        .path()
        .to_str()
        .ok_or_else(|| SyncError::Protocol("snapshot temporary path is not UTF-8".into()))?;
    // VACUUM INTO gives a consistent SQLite snapshot and removes free pages
    // left by an embedded-to-external conversion. It never copies a live file.
    ctx.catalog()
        .connection()
        .execute("VACUUM INTO ?1", [path])
        .map_err(CatalogError::from)?;
    if std::fs::metadata(temp.path())?.len() > protocol::MAX_SNAPSHOT_BYTES {
        return respond_text(request, 413, "snapshot exceeds transfer limit");
    }
    let c = icatalog_catalog::Catalog::open(temp.path())?;
    let row = c
        .catalogue()?
        .ok_or_else(|| SyncError::Protocol("server catalogue is not initialised".into()))?;
    let bytes = std::fs::read(temp.path())?;
    respond_bytes(
        request,
        200,
        bytes,
        Some((&row.id, row.generation)),
        "application/octet-stream",
    )
}
fn handle_edits(mut request: Request, ctx: &mut Context) -> Result<()> {
    if request.body_length().is_some_and(|n| n > MAX_SYNC_BYTES) {
        return respond_text(request, 413, "edit batch exceeds 64 MiB");
    }
    let mut bytes = Vec::new();
    request
        .as_reader()
        .take(MAX_SYNC_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_SYNC_BYTES {
        return respond_text(request, 413, "edit batch exceeds 64 MiB");
    }
    let (id, changes) = match delta::decode(&bytes) {
        Ok(v) => v,
        Err(e) => return respond_text(request, 400, &e.to_string()),
    };
    match apply_edits(ctx, &id, &changes) {
        Ok((generation, conflicts)) => respond_bytes(
            request,
            200,
            delta::encode_reply(generation, &conflicts)?,
            None,
            "application/octet-stream",
        ),
        Err(e) => respond_text(request, 400, &e.to_string()),
    }
}
/// Validate the complete batch first; apply non-conflicting entities in one transaction.
/// Retrying an already-applied value succeeds without changing its revision.
pub fn apply_edits(ctx: &Context, id: &str, changes: &[Change]) -> Result<(i64, Vec<String>)> {
    delta::validate(changes)?;
    if ctx.catalog().is_replica()? || ctx.catalog().catalogue()?.is_none_or(|r| r.id != id) {
        return Err(SyncError::Protocol(
            "wrong catalogue id or server is a replica".into(),
        ));
    }
    let c = ctx.catalog();
    c.connection()
        .execute_batch("BEGIN IMMEDIATE")
        .map_err(CatalogError::from)?;
    let result = (|| -> Result<(i64, Vec<String>)> {
        let current: BTreeMap<_, _> = delta::current(c)?
            .into_iter()
            .map(|r| (r.key(), r))
            .collect();
        let mut conflicts = Vec::new();
        for change in changes {
            let key = change.key();
            let previous = current.get(&key);
            if previous.is_some_and(|p| change.same_value(p)) {
                continue;
            }
            let matches = match previous {
                Some(p) => change.base_revision == Some(p.revision()),
                None => {
                    matches!(change.value, Value::Collection(_)) && change.base_revision.is_none()
                }
            };
            let mut missing_member = false;
            if let Value::Collection(v) = &change.value {
                for uid in &v.members {
                    if c.asset_by_uid(uid)?.is_none() {
                        missing_member = true;
                        break;
                    }
                }
            }
            let converted = matches!((&change.value, previous.map(|p| &p.value)), (Value::Collection(new), Some(Value::Collection(old))) if new.rules.is_some()!=old.rules.is_some());
            if !matches || missing_member || converted {
                conflicts.push(key);
                continue;
            }
            match &change.value {
                Value::Annotation(a) => {
                    c.apply_annotation_state(a, &icatalog_engine::utc_now_iso8601())?
                }
                Value::Collection(a) => c.apply_collection_state(a)?,
            }
        }
        let generation = c
            .generation()?
            .ok_or_else(|| SyncError::Protocol("uninitialised catalogue".into()))?;
        c.connection()
            .execute_batch("COMMIT")
            .map_err(CatalogError::from)?;
        Ok((generation, conflicts))
    })();
    if result.is_err() && !c.connection().is_autocommit() {
        c.connection()
            .execute_batch("ROLLBACK")
            .map_err(CatalogError::from)?;
    }
    result
}
fn respond_text(request: Request, status: u16, body: &str) -> Result<()> {
    respond_bytes(
        request,
        status,
        body.as_bytes().to_vec(),
        None,
        "text/plain; charset=utf-8",
    )
}
fn respond_bytes(
    request: Request,
    status: u16,
    body: Vec<u8>,
    info: Option<(&str, i64)>,
    content_type: &str,
) -> Result<()> {
    let mut response = Response::from_data(body).with_status_code(status);
    if let Some((id, generation)) = info {
        add_header(&mut response, HEADER_ID, id);
        add_header(&mut response, HEADER_GENERATION, &generation.to_string());
    }
    add_header(&mut response, "Content-Type", content_type);
    request.respond(response).map_err(SyncError::Io)?;
    Ok(())
}
fn add_header<R: Read>(response: &mut Response<R>, name: &str, value: &str) {
    if let Ok(h) = Header::from_bytes(name, value) {
        response.add_header(h);
    }
}
