mod engine;
mod find;
mod grep;
mod grep_result;
mod python;
mod python_errors;
mod python_types;
mod query;
#[cfg(test)]
mod tests;
mod types;

use pyo3::prelude::*;

#[derive(Debug)]
pub(crate) enum FffError {
    Configuration(String),
    Path(String),
    Query(String),
    Pattern(String),
    Limit(String),
    IndexNotReady,
    Closed,
    Cancelled,
    Runtime(String),
    Startup(String),
}

impl From<crate::workspace::WorkspaceError> for FffError {
    fn from(error: crate::workspace::WorkspaceError) -> Self {
        use crate::workspace::WorkspaceError as E;

        match error {
            E::Configuration(message) => Self::Configuration(message),
            E::Path(message) => Self::Path(message),
            E::Cancelled => Self::Cancelled,
            E::Limit(message) => Self::Limit(message),
            E::Closed => Self::Closed,
            E::Deadline => Self::IndexNotReady,
            other => Self::Runtime(other.into_message()),
        }
    }
}

pub(crate) fn register_module(module: &Bound<'_, PyModule>) -> PyResult<()> {
    python::register(module)
}
