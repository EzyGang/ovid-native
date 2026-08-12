use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::workspace::WorkspaceError;
use crate::workspace::path::resolve_contained_file;

pub(crate) struct PreparedWrite {
    target: PathBuf,
    permissions: fs::Permissions,
}

pub(crate) fn sha256(contents: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    let digest = Sha256::digest(contents);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        encoded.push(HEX[usize::from(byte >> 4)] as char);
        encoded.push(HEX[usize::from(byte & 0x0f)] as char);
    }

    encoded
}

pub(crate) fn preflight_write(
    root: &Path,
    relative: &str,
    expected_sha256: &str,
) -> Result<PreparedWrite, WorkspaceError> {
    let target = resolve_contained_file(root, relative)
        .map_err(|_| WorkspaceError::Stale(format!("rewrite target is unavailable: {relative}")))?;
    let contents = fs::read(&target)
        .map_err(|_| WorkspaceError::Stale(format!("cannot read rewrite target: {relative}")))?;
    if sha256(&contents) != expected_sha256 {
        return Err(WorkspaceError::Stale(format!(
            "rewrite target changed: {relative}"
        )));
    }
    let permissions = fs::metadata(&target)
        .map_err(|error| WorkspaceError::Write(format!("cannot inspect {relative}: {error}")))?
        .permissions();

    Ok(PreparedWrite {
        target,
        permissions,
    })
}

pub(crate) fn replace_file(
    prepared: &PreparedWrite,
    contents: &[u8],
) -> Result<(), WorkspaceError> {
    let parent = prepared
        .target
        .parent()
        .ok_or_else(|| WorkspaceError::Write("target has no parent directory".to_owned()))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|error| WorkspaceError::Write(error.to_string()))?;
    temporary
        .write_all(contents)
        .and_then(|()| temporary.flush())
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|error| WorkspaceError::Write(error.to_string()))?;
    temporary
        .as_file()
        .set_permissions(prepared.permissions.clone())
        .map_err(|error| WorkspaceError::Write(error.to_string()))?;
    temporary
        .persist(&prepared.target)
        .map_err(|error| WorkspaceError::Write(error.error.to_string()))?;

    Ok(())
}
