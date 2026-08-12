mod language;
mod python;
mod rewrite;
mod scanner;
mod search;
#[cfg(test)]
mod tests;
mod types;

use std::path::{Path, PathBuf};

use pyo3::prelude::*;
use sha2::{Digest, Sha256};

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

pub fn canonical_root(value: &str) -> Result<PathBuf, AstError> {
    if value.contains('\0') {
        return Err(AstError::Configuration(
            "workspace root contains a NUL byte".to_owned(),
        ));
    }
    let root = Path::new(value).canonicalize().map_err(|error| {
        AstError::Configuration(format!("cannot resolve workspace root: {error}"))
    })?;
    if !root.is_dir() {
        return Err(AstError::Configuration(
            "workspace root must be a directory".to_owned(),
        ));
    }
    Ok(root)
}

pub fn sha256(contents: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    let digest = Sha256::digest(contents);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        encoded.push(HEX[usize::from(byte >> 4)] as char);
        encoded.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    encoded
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
