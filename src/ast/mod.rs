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
            crate::workspace::WorkspaceError::Path(message) => Self::Path(message),
            crate::workspace::WorkspaceError::Read(message) => Self::Path(message),
            crate::workspace::WorkspaceError::Stale(message) => Self::Stale(message),
            crate::workspace::WorkspaceError::Write(message) => Self::Write(message),
            crate::workspace::WorkspaceError::Cancelled => Self::Cancelled,
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
