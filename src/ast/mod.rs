mod language;
mod python;
mod rewrite;
mod search;
#[cfg(test)]
mod tests;
mod types;

use pyo3::prelude::*;

pub use python::register;

#[derive(Debug)]
pub enum AstError {
    Configuration(String),
    Path(String),
    Language(String),
    Pattern(String),
    Limit(String),
    Stale(String),
    Write(String),
    Cancelled,
}

impl From<crate::workspace::WorkspaceError> for AstError {
    fn from(error: crate::workspace::WorkspaceError) -> Self {
        match error {
            crate::workspace::WorkspaceError::Configuration(message) => {
                Self::Configuration(message)
            }
            crate::workspace::WorkspaceError::Path(message)
            | crate::workspace::WorkspaceError::Read(message)
            | crate::workspace::WorkspaceError::Encoding(message)
            | crate::workspace::WorkspaceError::Binary(message) => Self::Path(message),
            crate::workspace::WorkspaceError::Limit(message) => Self::Limit(message),
            crate::workspace::WorkspaceError::ObservationNotFound(message)
            | crate::workspace::WorkspaceError::ObservationCollision(message)
            | crate::workspace::WorkspaceError::UnseenLine(message)
            | crate::workspace::WorkspaceError::ObservedLineChanged(message)
            | crate::workspace::WorkspaceError::Stale(message) => Self::Stale(message),
            crate::workspace::WorkspaceError::EditMode(message)
            | crate::workspace::WorkspaceError::Patch(message) => Self::Pattern(message),
            crate::workspace::WorkspaceError::PartialCommit {
                landed, pending, ..
            } => Self::Write(format!(
                "workspace operation committed [{}] with pending [{}]",
                landed.join(", "),
                pending.join(", ")
            )),
            crate::workspace::WorkspaceError::Write(message) => Self::Write(message),
            crate::workspace::WorkspaceError::Cancelled => Self::Cancelled,
            crate::workspace::WorkspaceError::Closed => {
                Self::Stale("workspace is closed".to_owned())
            }
            crate::workspace::WorkspaceError::Deadline => {
                Self::Limit("workspace operation reached its deadline".to_owned())
            }
        }
    }
}

pub fn source_range(source: &str, range: std::ops::Range<usize>) -> types::Range {
    (position(source, range.start), position(source, range.end))
}

pub fn register_module(module: &Bound<'_, PyModule>) -> PyResult<()> {
    register(module)
}

fn position(source: &str, offset: usize) -> types::Position {
    let prefix = &source[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix
        .rsplit_once('\n')
        .map_or(prefix, |(_, current_line)| current_line)
        .chars()
        .count()
        + 1;
    (line, column, offset)
}
