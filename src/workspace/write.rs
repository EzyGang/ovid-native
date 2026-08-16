use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::workspace::content::NormalizedText;

use crate::workspace::WorkspaceError;
use crate::workspace::control::Cancellation;
use crate::workspace::path::resolve_contained_file;

struct PreparedWrite {
    target: PathBuf,
    permissions: fs::Permissions,
}

#[derive(Clone, Debug)]
pub(crate) struct FileChange {
    pub path: String,
    pub operation: String,
    pub destination: Option<String>,
    pub before_sha256: Option<String>,
    pub after_sha256: Option<String>,
    pub observation: Option<crate::workspace::ObservationReceipt>,
    pub file_generation: u64,
    pub revision: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct PostEditSource {
    pub path: String,
    pub observation: crate::workspace::ObservationReceipt,
    pub lines: Vec<crate::workspace::RenderedLine>,
    pub complete_presentation: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct EditResult {
    pub mode: String,
    pub mode_generation: u64,
    pub policy_generation: u64,
    pub changes: Vec<FileChange>,
    pub post_edit_sources: Vec<PostEditSource>,
    pub preflight_complete: bool,
    pub commit_complete: bool,
    pub matching_strategy: Option<String>,
    pub confidence: Option<f64>,
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
) -> Result<(), WorkspaceError> {
    prepare_write(root, relative, expected_sha256).map(drop)
}

pub(crate) fn replace_file(
    root: &Path,
    relative: &str,
    expected_sha256: &str,
    contents: &[u8],
    cancellation: &Cancellation,
) -> Result<(), WorkspaceError> {
    if cancellation.is_cancelled() {
        return Err(WorkspaceError::Cancelled);
    }
    let prepared = prepare_write(root, relative, expected_sha256)?;
    if cancellation.is_cancelled() {
        return Err(WorkspaceError::Cancelled);
    }

    replace_prepared_file(&prepared, contents)
}

pub(crate) fn create_file(
    root: &Path,
    relative: &str,
    contents: &[u8],
    create_parents: bool,
) -> Result<PathBuf, WorkspaceError> {
    let target = crate::workspace::path::resolve_new_file(root, relative, create_parents)?;
    let parent = target
        .parent()
        .ok_or_else(|| WorkspaceError::Write(format!("path has no parent: {relative}")))?;
    if create_parents {
        fs::create_dir_all(parent).map_err(|error| {
            WorkspaceError::Write(format!(
                "cannot create parent directories for {relative}: {error}"
            ))
        })?;
    }
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|error| WorkspaceError::Write(format!("cannot prepare {relative}: {error}")))?;
    temporary
        .write_all(contents)
        .and_then(|()| temporary.flush())
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|error| WorkspaceError::Write(format!("cannot write {relative}: {error}")))?;
    temporary.persist_noclobber(&target).map_err(|error| {
        WorkspaceError::Write(format!(
            "cannot commit new workspace file {relative}: {}",
            error.error
        ))
    })?;
    Ok(target)
}

pub(crate) fn atomic_replace_path(
    target: &Path,
    relative: &str,
    expected_sha256: &str,
    contents: &[u8],
) -> Result<(), WorkspaceError> {
    let current = fs::read(target)
        .map_err(|_| WorkspaceError::Stale(format!("cannot read rewrite target: {relative}")))?;
    if sha256(NormalizedText::decode(current)?.source.as_bytes()) != expected_sha256 {
        return Err(WorkspaceError::Stale(format!(
            "rewrite target changed: {relative}"
        )));
    }
    let permissions = fs::metadata(target)
        .map_err(|error| WorkspaceError::Write(format!("cannot inspect {relative}: {error}")))?
        .permissions();
    replace_prepared_file(
        &PreparedWrite {
            target: target.to_path_buf(),
            permissions,
        },
        contents,
    )
    .map_err(|error| match error {
        WorkspaceError::Write(message) => {
            WorkspaceError::Write(format!("cannot replace {relative}: {message}"))
        }
        other => other,
    })
}

pub(crate) fn move_file_noclobber(
    source: &Path,
    destination: &Path,
    relative: &str,
    destination_relative: &str,
) -> Result<(), WorkspaceError> {
    fs::hard_link(source, destination).map_err(|error| {
        WorkspaceError::Write(format!(
            "cannot move {relative} to {destination_relative}: {error}"
        ))
    })?;
    fs::remove_file(source).map_err(|_| WorkspaceError::PartialCommit {
        landed: vec![destination_relative.to_owned()],
        pending: vec![relative.to_owned()],
    })
}

fn prepare_write(
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

fn replace_prepared_file(prepared: &PreparedWrite, contents: &[u8]) -> Result<(), WorkspaceError> {
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
