use std::path::Path;
use std::sync::Arc;

use crate::ast::AstError;
use crate::ast::types::{ApplyResult, FileChange, RewriteComputation};
use crate::workspace::{Cancellation, WorkspaceError, preflight_write, replace_file};

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
    let mut stale = Vec::new();
    for file in &computation.files {
        if cancellation.is_cancelled() {
            return Err(AstError::Cancelled);
        }
        match preflight_write(root, &file.path, &file.original_sha256) {
            Ok(()) => (),
            Err(WorkspaceError::Stale(_)) => stale.push(file.path.clone()),
            Err(error) => return Err(error.into()),
        }
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
    for file in &computation.files {
        if cancellation.is_cancelled() {
            return Err(AstError::Cancelled);
        }
        let result = replace_file(
            root,
            &file.path,
            &file.original_sha256,
            file.updated.as_bytes(),
            cancellation,
        );
        match result {
            Ok(()) => (),
            Err(WorkspaceError::Stale(message)) if applied.is_empty() => {
                return Err(AstError::Stale(message));
            }
            Err(error) => {
                let applied_paths = if applied.is_empty() {
                    "none".to_owned()
                } else {
                    applied.join(", ")
                };
                return Err(AstError::Write(format!(
                    "failed to replace {} after writing {applied_paths}: {error:?}",
                    file.path
                )));
            }
        }
        applied.push(file.path.clone());
    }

    let files = computation.files.iter().map(file_change).collect();
    Ok((files, computation.total_replacements))
}

fn file_change(file: &crate::ast::types::FileComputation) -> FileChange {
    (
        file.path.clone(),
        file.original_sha256.clone(),
        file.updated_sha256.clone(),
        file.replacements,
    )
}
