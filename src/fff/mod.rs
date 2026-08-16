mod engine;
mod find;
mod grep;
mod python;
mod python_errors;
mod python_types;
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
        match error {
            crate::workspace::WorkspaceError::Configuration(message) => {
                Self::Configuration(message)
            }
            crate::workspace::WorkspaceError::Path(message) => Self::Path(message),
            crate::workspace::WorkspaceError::Cancelled => Self::Cancelled,
            crate::workspace::WorkspaceError::Read(message)
            | crate::workspace::WorkspaceError::Stale(message)
            | crate::workspace::WorkspaceError::Write(message) => Self::Runtime(message),
            crate::workspace::WorkspaceError::Closed => Self::Closed,
            crate::workspace::WorkspaceError::Deadline => Self::IndexNotReady,
        }
    }
}

pub(crate) fn register_module(module: &Bound<'_, PyModule>) -> PyResult<()> {
    python::register(module)
}
