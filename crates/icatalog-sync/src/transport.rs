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
    /// Bounded, single-line diagnostics from an HTTP error response. A legacy
    /// server may send an empty 500, so always include the status and next step.
    pub(crate) fn status_error(&self, request: &str) -> SyncError {
        if matches!(self.status, 401 | 403) {
            return SyncError::Http(format!(
                "{request} returned HTTP {}; authentication failed. Check the selected token file and server access settings",
                self.status
            ));
        }
        let prefix = self
            .body
            .get(..self.body.len().min(2048))
            .unwrap_or_default();
        let detail: String = String::from_utf8_lossy(prefix)
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        let detail = detail.trim();
        SyncError::Http(format!(
            "{request} returned HTTP {}. Check the sync server's terminal for details.{}{}",
            self.status,
            if detail.is_empty() {
                ""
            } else {
                " Server message: "
            },
            detail
        ))
    }

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
    fn get_with_progress(
        &self,
        url: &str,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<HttpResponse> {
        let response = self.get(url)?;
        let bytes = response.body.len() as u64;
        progress(bytes, Some(bytes));
        Ok(response)
    }
    fn post(&self, url: &str, body: &[u8]) -> Result<HttpResponse>;
}

/// HTTP(S) transport with verified TLS and optional origin-bound authentication.
pub struct UreqTransport {
    agent: ureq::Agent,
    token: Option<crate::security::BearerToken>,
    origin: Option<url::Origin>,
}

impl UreqTransport {
    pub fn new() -> Self {
        Self {
            agent: agent().build(),
            token: None,
            origin: None,
        }
    }

    /// Read local token/CA files before any request; secrets are never persisted.
    pub fn from_params(params: &icatalog_engine::Params, base: &str) -> Result<Self> {
        let url = crate::security::parse_url(base, false)?;
        let token = crate::security::BearerToken::from_params(params)?;
        if token.is_some()
            && url.scheme() != "https"
            && !crate::security::literal_loopback(&url, base)
        {
            return Err(icatalog_engine::EngineError::BadParam(
                "sending a token requires HTTPS except to literal loopback (127.0.0.1 or [::1])"
                    .into(),
            )
            .into());
        }
        if params.get("ca-file").is_some() && url.scheme() != "https" {
            return Err(icatalog_engine::EngineError::BadParam(
                "ca-file requires an HTTPS server URL".into(),
            )
            .into());
        }
        let mut builder = agent();
        if let Some(config) = crate::security::client_tls(params)? {
            builder = builder.tls_config(config);
        }
        Ok(Self {
            agent: builder.build(),
            token,
            origin: Some(url.origin()),
        })
    }

    fn request(&self, method: &str, address: &str) -> Result<ureq::Request> {
        let url = crate::security::parse_url(address, true)?;
        if self
            .origin
            .as_ref()
            .is_some_and(|origin| *origin != url.origin())
        {
            return Err(SyncError::Protocol(
                "request origin differs from the configured server".into(),
            ));
        }
        let mut request = self.agent.request(method, address);
        if let Some(token) = &self.token {
            request = request.set("Authorization", &token.authorization());
        }
        Ok(request)
    }
}

impl Default for UreqTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl Transport for UreqTransport {
    fn get(&self, url: &str) -> Result<HttpResponse> {
        let cap = if url.ends_with("/info") {
            8192
        } else if url.contains("/preview/") {
            icatalog_catalog::MAX_PREVIEW_BYTES as u64
        } else {
            MAX_SNAPSHOT_BYTES
        };
        finish(
            self.request("GET", url)?.call(),
            cap,
            self.token.is_some(),
            &mut |_, _| {},
        )
    }
    fn get_with_progress(
        &self,
        url: &str,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<HttpResponse> {
        let cap = if url.ends_with("/info") {
            8192
        } else if url.contains("/preview/") {
            icatalog_catalog::MAX_PREVIEW_BYTES as u64
        } else {
            MAX_SNAPSHOT_BYTES
        };
        finish(
            self.request("GET", url)?.call(),
            cap,
            self.token.is_some(),
            progress,
        )
    }
    fn post(&self, url: &str, body: &[u8]) -> Result<HttpResponse> {
        finish(
            self.request("POST", url)?.send_bytes(body),
            icatalog_catalog::MAX_SYNC_BYTES as u64,
            self.token.is_some(),
            &mut |_, _| {},
        )
    }
}

fn agent() -> ureq::AgentBuilder {
    ureq::AgentBuilder::new()
        .redirects(0)
        .timeout_connect(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(60))
}
fn finish(
    result: std::result::Result<ureq::Response, ureq::Error>,
    cap: u64,
    authenticated: bool,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<HttpResponse> {
    match result {
        Ok(resp) => read_response(resp, cap, authenticated, progress),
        // Non-2xx statuses are normal responses here (e.g. 409 on refusal).
        Err(ureq::Error::Status(_, resp)) => read_response(resp, cap, authenticated, progress),
        Err(ureq::Error::Transport(t)) => {
            // TLS validation/handshake failures are actionable errors, never an
            // offline success. Avoid returning URLs or third-party diagnostics.
            if t.message()
                .is_some_and(|message| message.starts_with("tls "))
            {
                Err(SyncError::Http("TLS connection failed; check the server certificate, its hostname and the trusted CA file".into()))
            } else {
                Err(SyncError::Unreachable(format!(
                    "connection failed ({:?}); check the server address and network",
                    t.kind()
                )))
            }
        }
    }
}

fn read_response(
    resp: ureq::Response,
    cap: u64,
    authenticated: bool,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<HttpResponse> {
    let status = resp.status();
    // Only 200 is a successful v2 protocol response. An error page or redirect
    // can echo Authorization, including through a proxy. Do not read or expose
    // its diagnostics when a credential was sent; preserve valid binary bodies.
    if authenticated && status != 200 {
        return Ok(HttpResponse {
            status,
            headers: BTreeMap::new(),
            body: Vec::new(),
        });
    }
    let mut headers = BTreeMap::new();
    for name in [HEADER_ID, HEADER_GENERATION, "Content-Length"] {
        if let Some(value) = resp.header(name) {
            headers.insert(name.to_ascii_lowercase(), value.to_string());
        }
    }
    let total = resp
        .header("Content-Length")
        .and_then(|value| value.trim().parse::<u64>().ok());
    if total.is_none() {
        headers.remove("content-length");
    }
    if total.is_some_and(|size| size > cap) {
        return Err(SyncError::Protocol(
            "response body exceeds the snapshot cap".into(),
        ));
    }
    let mut body = Vec::new();
    let mut reader = resp.into_reader().take(cap.saturating_add(1));
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let count = reader.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        let part = chunk
            .get(..count)
            .ok_or_else(|| SyncError::Protocol("invalid response chunk".into()))?;
        body.try_reserve(part.len())
            .map_err(|_| SyncError::Protocol("response is too large to buffer safely".into()))?;
        body.extend_from_slice(part);
        let completed = body.len() as u64;
        progress(completed, total);
        if completed > cap {
            break;
        }
    }
    if body.len() as u64 > cap {
        return Err(SyncError::Protocol(
            "response body exceeds the snapshot cap".into(),
        ));
    }
    if total.is_some_and(|size| size != body.len() as u64) {
        return Err(SyncError::Protocol(
            "response Content-Length does not match body".into(),
        ));
    }
    Ok(HttpResponse {
        status,
        headers,
        body,
    })
}
