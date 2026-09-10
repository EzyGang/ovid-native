mod glob;
mod grep;
mod grep_file;
mod grep_matcher;
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
        use crate::workspace::WorkspaceError as E;

        match error {
            E::Configuration(message) => Self::Configuration(message),
            E::Path(message) => Self::Path(message),
            E::Limit(message) => Self::Limit(message),
            E::Cancelled => Self::Cancelled,
            E::Deadline => Self::Limit("search operation reached its deadline".to_owned()),
            other => Self::Read(other.into_message()),
        }
    }
}

pub(crate) fn register_module(module: &Bound<'_, PyModule>) -> PyResult<()> {
    python::register(module)
}
