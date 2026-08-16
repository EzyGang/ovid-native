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
            crate::workspace::WorkspaceError::Read(message)
            | crate::workspace::WorkspaceError::Encoding(message)
            | crate::workspace::WorkspaceError::Binary(message)
            | crate::workspace::WorkspaceError::ObservationNotFound(message)
            | crate::workspace::WorkspaceError::ObservationCollision(message)
            | crate::workspace::WorkspaceError::UnseenLine(message)
            | crate::workspace::WorkspaceError::ObservedLineChanged(message)
            | crate::workspace::WorkspaceError::Stale(message)
            | crate::workspace::WorkspaceError::EditMode(message)
            | crate::workspace::WorkspaceError::Patch(message)
            | crate::workspace::WorkspaceError::Write(message) => Self::Read(message),
            crate::workspace::WorkspaceError::Limit(message) => Self::Limit(message),
            crate::workspace::WorkspaceError::PartialCommit { landed, pending } => {
                Self::Read(format!(
                    "workspace operation committed [{}] with pending [{}]",
                    landed.join(", "),
                    pending.join(", ")
                ))
            }
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
