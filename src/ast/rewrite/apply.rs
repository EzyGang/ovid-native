use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use crate::ast::types::{ApplyResult, Cancellation, FileChange, RewriteComputation};
use crate::ast::{AstError, sha256};

struct PreflightFile {
    target: PathBuf,
    permissions: fs::Permissions,
}

pub fn apply(
    root: &Path,
    computation: Arc<RewriteComputation>,
    cancellation: &Cancellation,
) -> Result<ApplyResult, AstError> {
    if computation.root != root {
        return Err(AstError::Configuration(
            "rewrite proposal belongs to a different workspace".to_owned(),
        ));
    }
    let mut preflight = Vec::with_capacity(computation.files.len());
    let mut stale = Vec::new();
    for file in &computation.files {
        if cancellation.is_cancelled() {
            return Err(AstError::Cancelled);
        }
        let target = resolve_target(root, &file.path)?;
        let contents = match fs::read(&target) {
            Ok(contents) => contents,
            Err(_) => {
                stale.push(file.path.clone());
                continue;
            }
        };
        if sha256(&contents) != file.original_sha256 {
            stale.push(file.path.clone());
            continue;
        }
        let permissions = fs::metadata(&target)
            .map_err(|error| AstError::Write(format!("cannot inspect {}: {error}", file.path)))?
            .permissions();
        preflight.push(PreflightFile {
            target,
            permissions,
        });
    }
    if !stale.is_empty() {
        return Err(AstError::Stale(format!(
            "rewrite proposal is stale for: {}",
            stale.join(", ")
        )));
    }
    if cancellation.is_cancelled() {
        return Err(AstError::Cancelled);
    }

    let mut applied = Vec::new();
    for (file, prepared) in computation.files.iter().zip(preflight) {
        if let Err(error) = write_file(&prepared, &file.updated) {
            let applied_paths = if applied.is_empty() {
                "none".to_owned()
            } else {
                applied.join(", ")
            };
            return Err(AstError::Write(format!(
                "failed to replace {} after writing {applied_paths}: {error}",
                file.path
            )));
        }
        applied.push(file.path.clone());
    }

    let files = computation.files.iter().map(file_change).collect();
    Ok((files, computation.total_replacements))
}

fn resolve_target(root: &Path, relative: &str) -> Result<PathBuf, AstError> {
    let relative_path = Path::new(relative);
    if relative_path.is_absolute()
        || relative_path
            .components()
            .any(|component| component == Component::ParentDir)
    {
        return Err(AstError::Path(format!(
            "rewrite path is outside the workspace: {relative}"
        )));
    }
    let target = root
        .join(relative_path)
        .canonicalize()
        .map_err(|error| AstError::Stale(format!("cannot resolve {relative}: {error}")))?;
    if !target.starts_with(root) {
        return Err(AstError::Path(format!(
            "rewrite path resolves outside the workspace: {relative}"
        )));
    }
    if !fs::metadata(&target).is_ok_and(|metadata| metadata.is_file()) {
        return Err(AstError::Stale(format!(
            "rewrite target is no longer a regular file: {relative}"
        )));
    }
    Ok(target)
}

fn write_file(file: &PreflightFile, contents: &str) -> Result<(), String> {
    let parent = file
        .target
        .parent()
        .ok_or_else(|| "target has no parent directory".to_owned())?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
    temporary
        .write_all(contents.as_bytes())
        .and_then(|()| temporary.flush())
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|error| error.to_string())?;
    temporary
        .as_file()
        .set_permissions(file.permissions.clone())
        .map_err(|error| error.to_string())?;
    temporary
        .persist(&file.target)
        .map_err(|error| error.error.to_string())?;
    Ok(())
}

fn file_change(file: &crate::ast::types::FileComputation) -> FileChange {
    (
        file.path.clone(),
        file.original_sha256.clone(),
        file.updated_sha256.clone(),
        file.replacements,
    )
}
