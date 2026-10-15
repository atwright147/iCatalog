use std::fmt;

use icatalog_catalog::CatalogError;
use icatalog_engine::EngineError;

/// Errors from the sync service and client.
#[derive(Debug)]
pub enum SyncError {
    /// Catalogue layer failure.
    Catalog(CatalogError),
    /// Engine plumbing failure.
    Engine(EngineError),
    /// Local filesystem or temp-file I/O failure.
    Io(std::io::Error),
    /// The server could not be reached (connection refused, DNS, timeout).
    Unreachable(String),
    /// A reachable server returned an unexpected response.
    Http(String),
    /// A response or snapshot did not follow the protocol.
    Protocol(String),
    /// A snapshot failed verification.
    Integrity(String),
}

/// Sync result type.
pub type Result<T> = std::result::Result<T, SyncError>;

impl fmt::Display for SyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SyncError::Catalog(e) => write!(f, "catalogue error: {e}"),
            SyncError::Engine(e) => write!(f, "{e}"),
            SyncError::Io(e) => write!(f, "I/O error: {e}"),
            SyncError::Unreachable(m) => write!(f, "server unreachable: {m}"),
            SyncError::Http(m) => write!(f, "http error: {m}"),
            SyncError::Protocol(m) => write!(f, "protocol error: {m}"),
            SyncError::Integrity(m) => write!(f, "integrity error: {m}"),
        }
    }
}

impl std::error::Error for SyncError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SyncError::Catalog(e) => Some(e),
            SyncError::Engine(e) => Some(e),
            SyncError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<CatalogError> for SyncError {
    fn from(e: CatalogError) -> Self {
        SyncError::Catalog(e)
    }
}

impl From<EngineError> for SyncError {
    fn from(e: EngineError) -> Self {
        SyncError::Engine(e)
    }
}

impl From<std::io::Error> for SyncError {
    fn from(e: std::io::Error) -> Self {
        SyncError::Io(e)
    }
}

impl From<SyncError> for EngineError {
    fn from(e: SyncError) -> Self {
        match e {
            SyncError::Engine(e) => e,
            SyncError::Catalog(e) => EngineError::Catalog(e),
            SyncError::Io(e) => EngineError::Io(e),
            SyncError::Unreachable(m) | SyncError::Protocol(m) | SyncError::Integrity(m) => {
                EngineError::Internal(m)
            }
            SyncError::Http(m) => EngineError::Internal(format!("http error: {m}")),
        }
    }
}
