//! The NAS-side sync service: a thin HTTP layer over an open [`Context`].
//! Handles `GET /info`, `GET /catalogue` and `POST /catalogue`.

use icatalog_engine::Context;
use tiny_http::{Header, Request, Response, Server};

use crate::error::{Result, SyncError};
use crate::protocol::{
    self, HEADER_GENERATION, HEADER_ID, MAX_SNAPSHOT_BYTES, PATH_CATALOGUE, PATH_INFO,
};
use crate::util;

/// Bind the service to `addr` (host:port; port 0 picks an ephemeral port).
pub fn bind(addr: &str) -> Result<Server> {
    Server::http(addr).map_err(|e| SyncError::Http(format!("cannot bind {addr}: {e}")))
}

/// Serve requests until the listener fails or is closed.
pub fn serve_loop(server: Server, ctx: &mut Context) -> Result<()> {
    loop {
        let request = server.recv().map_err(SyncError::Io)?;
        // A single bad request must never take the service down.
        if let Err(e) = handle(request, ctx) {
            eprintln!("icatalog serve: {e}");
        }
    }
}

fn handle(request: Request, ctx: &mut Context) -> Result<()> {
    let method = request.method().as_str().to_string();
    let url = request.url().to_string();
    let (path, query) = protocol::split_url(&url);
    match (method.as_str(), path) {
        ("GET", PATH_INFO) => respond_info(request, ctx),
        ("GET", PATH_CATALOGUE) => respond_snapshot(request, ctx),
        ("POST", PATH_CATALOGUE) => handle_push(request, ctx, query),
        _ => respond_text(request, 404, "not found"),
    }
}

fn respond_info(request: Request, ctx: &Context) -> Result<()> {
    let Some(row) = ctx.catalog().catalogue()? else {
        return respond_text(request, 500, "server catalogue is not initialised");
    };
    let info = protocol::CatalogueInfo {
        id: row.id,
        generation: row.generation,
    };
    respond_text(request, 200, &info.encode())
}

fn respond_snapshot(request: Request, ctx: &Context) -> Result<()> {
    let Some(row) = ctx.catalog().catalogue()? else {
        return respond_text(request, 500, "server catalogue is not initialised");
    };
    let bytes = std::fs::read(ctx.path())?;
    let mut resp = Response::from_data(bytes).with_status_code(200);
    add_header(&mut resp, HEADER_ID, &row.id);
    add_header(&mut resp, HEADER_GENERATION, &row.generation.to_string());
    add_header(&mut resp, "Content-Type", "application/octet-stream");
    request.respond(resp).map_err(SyncError::Io)?;
    Ok(())
}

fn handle_push(mut request: Request, ctx: &mut Context, query: &str) -> Result<()> {
    let Some(row) = ctx.catalog().catalogue()? else {
        return respond_text(request, 500, "server catalogue is not initialised");
    };
    let expected = match protocol::query_param(query, "expected") {
        Some(v) => match v.trim().parse::<i64>() {
            Ok(n) => n,
            Err(_) => return respond_text(request, 400, "bad 'expected' generation"),
        },
        None => return respond_text(request, 400, "missing 'expected' generation"),
    };

    // Stream the body to a temp file beside the canonical copy, bounded.
    let temp = util::temp_path(&util::parent_dir(ctx.path()), "incoming");
    if let Err(e) = read_body_to_file(&mut request, &temp) {
        let _ = std::fs::remove_file(&temp);
        return respond_text(request, 413, &format!("snapshot rejected: {e}"));
    }

    if expected != row.generation {
        let _ = std::fs::remove_file(&temp);
        return respond_generation(request, 409, row.generation);
    }
    if let Err(e) = util::verify_catalogue(&temp, &row.id) {
        let _ = std::fs::remove_file(&temp);
        return respond_text(request, 400, &format!("snapshot rejected: {e}"));
    }
    if let Err(e) = ctx.replace_catalogue_file(&temp) {
        let _ = std::fs::remove_file(&temp);
        return Err(e.into());
    }
    let generation = ctx.catalog().bump_generation()?;
    respond_generation(request, 200, generation)
}

/// Copy a bounded request body to `path`; reject anything over the cap.
fn read_body_to_file(request: &mut Request, path: &std::path::Path) -> Result<u64> {
    if let Some(len) = request.body_length() {
        if len as u64 > MAX_SNAPSHOT_BYTES {
            return Err(SyncError::Http(format!(
                "body is {len} bytes, over the cap"
            )));
        }
    }
    let mut file = std::fs::File::create(path)?;
    let limit = MAX_SNAPSHOT_BYTES.saturating_add(1);
    let mut body = std::io::Read::take(request.as_reader(), limit);
    let written = std::io::copy(&mut body, &mut file)?;
    if written > MAX_SNAPSHOT_BYTES {
        return Err(SyncError::Http("body exceeded the cap".into()));
    }
    Ok(written)
}

fn respond_generation(request: Request, status: u16, generation: i64) -> Result<()> {
    let mut resp = Response::empty(status);
    add_header(&mut resp, HEADER_GENERATION, &generation.to_string());
    request.respond(resp).map_err(SyncError::Io)?;
    Ok(())
}

fn respond_text(request: Request, status: u16, body: &str) -> Result<()> {
    let mut resp = Response::from_string(body.to_owned()).with_status_code(status);
    add_header(&mut resp, "Content-Type", "text/plain; charset=utf-8");
    request.respond(resp).map_err(SyncError::Io)?;
    Ok(())
}

fn add_header<R: std::io::Read>(resp: &mut Response<R>, name: &str, value: &str) {
    if let Ok(header) = Header::from_bytes(name, value) {
        resp.add_header(header);
    }
}
