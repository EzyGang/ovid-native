mod glob;
mod grep;
mod python;
#[cfg(test)]
mod tests;
mod types;

use pyo3::prelude::*;

#[derive(Debug)]
pub(crate) enum SearchError {
    Configuration(String),
    Path(String),
    Pattern(String),
    Limit(String),
    Cancelled,
    Read(String),
}

impl From<crate::workspace::WorkspaceError> for SearchError {
    fn from(error: crate::workspace::WorkspaceError) -> Self {
        match error {
            crate::workspace::WorkspaceError::Configuration(message) => {
                Self::Configuration(message)
            }
            crate::workspace::WorkspaceError::Path(message) => Self::Path(message),
            crate::workspace::WorkspaceError::Read(message) => Self::Read(message),
            crate::workspace::WorkspaceError::Stale(message)
            | crate::workspace::WorkspaceError::Write(message) => Self::Read(message),
            crate::workspace::WorkspaceError::Closed => {
                Self::Read("workspace is closed".to_owned())
            }
            crate::workspace::WorkspaceError::Cancelled => Self::Cancelled,
            crate::workspace::WorkspaceError::Deadline => {
                Self::Limit("search operation reached its deadline".to_owned())
            }
        }
    }
}

pub(crate) fn register_module(module: &Bound<'_, PyModule>) -> PyResult<()> {
    python::register(module)
}
