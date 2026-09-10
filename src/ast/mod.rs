mod language;
mod matches;
mod python;
mod rewrite;
mod search;
mod source;
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
        use crate::workspace::WorkspaceError as E;

        match error {
            E::Configuration(message) => Self::Configuration(message),
            E::Path(message) | E::Read(message) | E::Encoding(message) | E::Binary(message) => {
                Self::Path(message)
            }
            E::Limit(message) => Self::Limit(message),
            E::EditMode(message) | E::Patch(message) => Self::Pattern(message),
            E::PartialCommit { .. } | E::Write(_) => Self::Write(error.into_message()),
            E::Cancelled => Self::Cancelled,
            E::Deadline => Self::Limit("workspace operation reached its deadline".to_owned()),
            other => Self::Stale(other.into_message()),
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
