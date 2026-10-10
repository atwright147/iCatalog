use std::fmt;

/// Errors returned by engine commands.
#[derive(Debug)]
pub enum EngineError {
    /// Catalogue layer failure.
    Catalog(icatalog_catalog::CatalogError),
    /// A missing or malformed parameter.
    BadParam(String),
    /// I/O failure.
    Io(std::io::Error),
    /// No command with this id.
    UnknownCommand(String),
    /// Operation not supported in the current state.
    Unsupported(String),
    /// Internal failure (e.g. a caught panic).
    Internal(String),
}

/// Engine result type.
pub type Result<T> = std::result::Result<T, EngineError>;

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::Catalog(e) => write!(f, "catalogue error: {e}"),
            EngineError::BadParam(m) => write!(f, "bad parameter: {m}"),
            EngineError::Io(e) => write!(f, "I/O error: {e}"),
            EngineError::UnknownCommand(id) => write!(f, "unknown command '{id}'"),
            EngineError::Unsupported(m) => write!(f, "unsupported: {m}"),
            EngineError::Internal(m) => write!(f, "internal error: {m}"),
        }
    }
}

impl std::error::Error for EngineError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            EngineError::Catalog(e) => Some(e),
            EngineError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<icatalog_catalog::CatalogError> for EngineError {
    fn from(e: icatalog_catalog::CatalogError) -> Self {
        EngineError::Catalog(e)
    }
}

impl From<std::io::Error> for EngineError {
    fn from(e: std::io::Error) -> Self {
        EngineError::Io(e)
    }
}
