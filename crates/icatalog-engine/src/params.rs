use crate::error::{EngineError, Result};
use std::collections::BTreeMap;

/// String key/value parameters for a command.
#[derive(Debug, Clone, Default)]
pub struct Params {
    values: BTreeMap<String, String>,
}

impl Params {
    /// Empty parameter set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a parameter; returns self for chaining.
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) -> &mut Self {
        self.values.insert(key.into(), value.into());
        self
    }

    /// Get a parameter if present.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    /// Get a required parameter.
    pub fn require(&self, key: &str) -> Result<&str> {
        self.get(key)
            .ok_or_else(|| EngineError::BadParam(format!("missing required parameter '{key}'")))
    }

    /// Get an optional integer parameter.
    pub fn get_i64(&self, key: &str) -> Result<Option<i64>> {
        match self.get(key) {
            None => Ok(None),
            Some(v) => v.parse::<i64>().map(Some).map_err(|_| {
                EngineError::BadParam(format!("parameter '{key}' must be an integer"))
            }),
        }
    }

    /// True if no parameters are set.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}
