//! The client-side transport seam. A native blocking HTTP implementation
//! (`ureq`) lives here; a wasm `fetch` implementation can slot in later
//! behind the same trait (AGENTS wasm rule).

use std::collections::BTreeMap;
use std::io::Read;

use crate::error::{Result, SyncError};
use crate::protocol::{HEADER_GENERATION, HEADER_ID, MAX_SNAPSHOT_BYTES};

/// A minimal HTTP response: status, selected headers and the body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    /// Case-insensitive header lookup.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }
}

/// A blocking HTTP transport (GET and POST).
pub trait Transport {
    fn get(&self, url: &str) -> Result<HttpResponse>;
    fn post(&self, url: &str, body: &[u8]) -> Result<HttpResponse>;
}

/// Plain-HTTP transport over `ureq` (no TLS in this slice).
pub struct UreqTransport;

impl UreqTransport {
    pub fn new() -> Self {
        UreqTransport
    }
}

impl Default for UreqTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl Transport for UreqTransport {
    fn get(&self, url: &str) -> Result<HttpResponse> {
        finish(ureq::get(url).call())
    }
    fn post(&self, url: &str, body: &[u8]) -> Result<HttpResponse> {
        finish(ureq::post(url).send_bytes(body))
    }
}

fn finish(result: std::result::Result<ureq::Response, ureq::Error>) -> Result<HttpResponse> {
    match result {
        Ok(resp) => read_response(resp),
        // Non-2xx statuses are normal responses here (e.g. 409 on refusal).
        Err(ureq::Error::Status(_, resp)) => read_response(resp),
        Err(ureq::Error::Transport(t)) => Err(SyncError::Unreachable(t.to_string())),
    }
}

fn read_response(resp: ureq::Response) -> Result<HttpResponse> {
    let status = resp.status();
    let mut headers = BTreeMap::new();
    for name in [HEADER_ID, HEADER_GENERATION] {
        if let Some(value) = resp.header(name) {
            headers.insert(name.to_ascii_lowercase(), value.to_string());
        }
    }
    let mut body = Vec::new();
    resp.into_reader()
        .take(MAX_SNAPSHOT_BYTES.saturating_add(1))
        .read_to_end(&mut body)?;
    if body.len() as u64 > MAX_SNAPSHOT_BYTES {
        return Err(SyncError::Protocol(
            "response body exceeds the snapshot cap".into(),
        ));
    }
    Ok(HttpResponse {
        status,
        headers,
        body,
    })
}
