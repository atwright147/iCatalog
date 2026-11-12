//! Local transport credentials. Secrets never enter the catalogue or command output.
use crate::{Result, SyncError};
use icatalog_engine::{EngineError, ParamSpec, Params};
use std::io::Read;
use std::path::Path;
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

const MAX_TOKEN_BYTES: usize = 4096;
const MAX_PEM_BYTES: usize = 1024 * 1024;

pub(crate) const TOKEN_FILE: ParamSpec = ParamSpec {
    name: "token-file",
    required: false,
    positional: false,
    doc: "file containing a shared bearer token (32–4096 ASCII token characters); HTTPS required except literal loopback",
};
pub(crate) const CA_FILE: ParamSpec = ParamSpec {
    name: "ca-file",
    required: false,
    positional: false,
    doc: "PEM CA certificate bundle to trust in addition to bundled public roots (HTTPS only)",
};

fn invalid(message: &str) -> SyncError {
    EngineError::BadParam(message.into()).into()
}

pub(crate) fn read_file(path: &Path, limit: usize, label: &str) -> Result<Vec<u8>> {
    // Probe before opening so ordinary FIFOs/devices are never opened for blocking reads.
    let metadata = std::fs::metadata(path).map_err(|_| {
        invalid(&format!(
            "cannot read {label}; check the selected file and permissions"
        ))
    })?;
    if !metadata.is_file() || metadata.len() > limit as u64 {
        return Err(invalid(&format!(
            "{label} must be a regular file of at most {limit} bytes"
        )));
    }
    let file = std::fs::File::open(path).map_err(|_| {
        invalid(&format!(
            "cannot open {label}; check the selected file and permissions"
        ))
    })?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > limit as u64 {
        return Err(invalid(&format!(
            "{label} must be a regular file of at most {limit} bytes"
        )));
    }
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid(&format!("cannot read {label}")))?;
    if bytes.len() > limit {
        return Err(invalid(&format!("{label} exceeds {limit} bytes")));
    }
    Ok(bytes)
}

/// A validated shared secret. Debug and Display deliberately are not implemented.
pub struct BearerToken(Zeroizing<String>);
impl BearerToken {
    pub fn from_file(path: &Path) -> Result<Self> {
        let bytes = Zeroizing::new(read_file(path, MAX_TOKEN_BYTES + 2, "token file")?);
        let token = std::str::from_utf8(&bytes)
            .map_err(|_| invalid("token file must contain ASCII token characters"))?;
        let token = token
            .strip_suffix("\r\n")
            .or_else(|| token.strip_suffix('\n'))
            .unwrap_or(token);
        if !(32..=MAX_TOKEN_BYTES).contains(&token.len())
            || !token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-._~+/=".contains(&b))
        {
            return Err(invalid("token file must contain 32–4096 ASCII letters, digits or -._~+/=, optionally followed by one newline"));
        }
        Ok(Self(Zeroizing::new(token.to_owned())))
    }
    pub(crate) fn from_params(params: &Params) -> Result<Option<Self>> {
        params
            .get("token-file")
            .map(|p| Self::from_file(Path::new(p)))
            .transpose()
    }
    pub(crate) fn authorization(&self) -> Zeroizing<String> {
        Zeroizing::new(format!("Bearer {}", self.0.as_str()))
    }
    pub(crate) fn matches(&self, value: &str) -> bool {
        let Some((scheme, candidate)) = value.split_once(' ') else {
            return false;
        };
        scheme.eq_ignore_ascii_case("Bearer")
            && bool::from(self.0.as_bytes().ct_eq(candidate.as_bytes()))
    }
}

/// Validate a server base URL without echoing potentially secret URL text.
pub(crate) fn parse_url(base: &str, allow_query: bool) -> Result<url::Url> {
    if base.is_empty()
        || base.len() > 8192
        || base
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '\\')
        || !(base.starts_with("http://") || base.starts_with("https://"))
    {
        return Err(invalid("server URL must be an HTTP or HTTPS address of at most 8 KiB, without whitespace or backslashes"));
    }
    let parsed = url::Url::parse(base).map_err(|_| invalid("invalid server URL"))?;
    // URL parsers normalize an empty username away; still reject an explicit @.
    let authority = base
        .split_once("://")
        .map(|(_, tail)| tail.split(['/', '?', '#']).next().unwrap_or(""))
        .unwrap_or("");
    if authority.is_empty()
        || authority.contains('@')
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.fragment().is_some()
        || (!allow_query && parsed.query().is_some())
    {
        return Err(invalid(
            "server URL must have a host and must not contain credentials, a query or a fragment",
        ));
    }
    Ok(parsed)
}

pub(crate) fn literal_loopback(url: &url::Url, raw: &str) -> bool {
    let authority = raw
        .split_once("://")
        .map(|(_, tail)| tail.split('/').next().unwrap_or(""))
        .unwrap_or("");
    match url.host() {
        Some(url::Host::Ipv4(ip)) => {
            ip == std::net::Ipv4Addr::LOCALHOST
                && (authority == "127.0.0.1" || authority.starts_with("127.0.0.1:"))
        }
        Some(url::Host::Ipv6(ip)) => {
            ip == std::net::Ipv6Addr::LOCALHOST
                && (authority == "[::1]" || authority.starts_with("[::1]:"))
        }
        _ => false,
    }
}

pub(crate) fn client_tls(params: &Params) -> Result<Option<std::sync::Arc<rustls::ClientConfig>>> {
    let Some(path) = params.get("ca-file") else {
        return Ok(None);
    };
    let bytes = read_file(Path::new(path), MAX_PEM_BYTES, "CA certificate file")?;
    let certificates = rustls_pemfile::certs(&mut bytes.as_slice())
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|_| invalid("CA certificate file is not valid PEM"))?;
    if certificates.is_empty() {
        return Err(invalid("CA certificate file contains no certificates"));
    }
    let mut roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    for certificate in certificates {
        roots
            .add(certificate)
            .map_err(|_| invalid("CA certificate file contains an invalid certificate"))?;
    }
    let config = rustls::ClientConfig::builder_with_provider(
        rustls::crypto::ring::default_provider().into(),
    )
    .with_safe_default_protocol_versions()
    .map_err(|_| invalid("cannot configure TLS protocol versions"))?
    .with_root_certificates(roots)
    .with_no_client_auth();
    Ok(Some(std::sync::Arc::new(config)))
}

/// Server settings loaded once before binding; token rotation requires restarting serve.
pub struct ServerSecurity {
    pub(crate) token: Option<BearerToken>,
    pub(crate) tls: Option<tiny_http::SslConfig>,
}
impl ServerSecurity {
    pub fn from_params(params: &Params, bind: &str) -> Result<Self> {
        let token = BearerToken::from_params(params)?;
        let tls = match (params.get("tls-cert"), params.get("tls-key")) {
            (None, None) => None,
            (Some(cert), Some(key)) => Some(server_tls(Path::new(cert), Path::new(key))?),
            _ => return Err(invalid("tls-cert and tls-key must be supplied together")),
        };
        if token.is_some() && tls.is_none() && !matches!(bind, "127.0.0.1" | "::1" | "[::1]") {
            return Err(invalid("serving with a token requires TLS, except on literal loopback for a local TLS proxy"));
        }
        Ok(Self { token, tls })
    }
    pub fn uses_tls(&self) -> bool {
        self.tls.is_some()
    }
}

fn server_tls(cert: &Path, key: &Path) -> Result<tiny_http::SslConfig> {
    let certificate = read_file(cert, MAX_PEM_BYTES, "TLS certificate file")?;
    let private_key = Zeroizing::new(read_file(key, MAX_PEM_BYTES, "TLS private key file")?);
    let certificates = rustls_pemfile::certs(&mut certificate.as_slice())
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|_| invalid("TLS certificate file is not valid PEM"))?;
    if certificates.is_empty() {
        return Err(invalid("TLS certificate file contains no certificates"));
    }
    let mut reader = private_key.as_slice();
    let key = rustls_pemfile::private_key(&mut reader)
        .map_err(|_| invalid("TLS private key must be unencrypted PEM"))?
        .ok_or_else(|| invalid("TLS private key file contains no supported private key"))?;
    if rustls_pemfile::private_key(&mut reader)
        .map_err(|_| invalid("TLS private key must be unencrypted PEM"))?
        .is_some()
    {
        return Err(invalid("TLS private key file must contain exactly one key"));
    }
    rustls::ServerConfig::builder_with_provider(rustls::crypto::ring::default_provider().into())
        .with_safe_default_protocol_versions()
        .map_err(|_| invalid("cannot configure TLS protocol versions"))?
        .with_no_client_auth()
        .with_single_cert(certificates, key)
        .map_err(|_| invalid("TLS certificate and private key are invalid or do not match"))?;
    Ok(tiny_http::SslConfig {
        certificate,
        private_key: private_key.to_vec(),
    })
}
