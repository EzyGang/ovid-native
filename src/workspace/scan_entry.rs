use std::{
    fs,
    path::{Path, PathBuf},
};

use ignore::DirEntry;

use crate::workspace::path::relative_path;
use crate::workspace::selection::{Selection, explicitly_selected_file, selected};
use crate::workspace::{
    MetadataLevel, ScanFileKind, WorkspaceEntry, WorkspaceError, WorkspaceFileType,
};

pub(super) struct ScanCandidate {
    entry: DirEntry,
    relative: String,
    file_type: WorkspaceFileType,
    followed_metadata: Option<fs::Metadata>,
}

impl ScanCandidate {
    pub(super) fn select(
        root: &Path,
        entry: DirEntry,
        selections: &[Selection],
        requested_kind: ScanFileKind,
    ) -> Result<Option<Self>, WorkspaceError> {
        let relative = relative_path(root, entry.path())?;
        if relative.is_empty() || !selected(&relative, selections) {
            return Ok(None);
        }
        let native_file_type = entry.file_type().ok_or_else(|| {
            WorkspaceError::Path(format!("cannot classify workspace path {relative}"))
        })?;
        let is_symlink = native_file_type.is_symlink();
        if is_symlink && !explicitly_selected_file(&relative, selections) {
            return Ok(None);
        }
        let followed_metadata = if is_symlink {
            Some(fs::metadata(entry.path()).map_err(|error| {
                WorkspaceError::Path(format!("cannot inspect workspace path {relative}: {error}"))
            })?)
        } else {
            None
        };
        let file_type = match (
            followed_metadata
                .as_ref()
                .is_some_and(fs::Metadata::is_file)
                || native_file_type.is_file(),
            followed_metadata.as_ref().is_some_and(fs::Metadata::is_dir)
                || native_file_type.is_dir(),
        ) {
            (true, false) if requested_kind != ScanFileKind::Directories => WorkspaceFileType::File,
            (false, true) if requested_kind != ScanFileKind::Files => WorkspaceFileType::Directory,
            _ => return Ok(None),
        };
        Ok(Some(Self {
            entry,
            relative,
            file_type,
            followed_metadata,
        }))
    }

    pub(super) fn identity(&self, root: &Path) -> Result<PathBuf, WorkspaceError> {
        if self.followed_metadata.is_none() {
            return Ok(self.entry.path().to_path_buf());
        }
        let canonical = self.entry.path().canonicalize().map_err(|error| {
            WorkspaceError::Path(format!(
                "cannot resolve workspace path {}: {error}",
                self.relative
            ))
        })?;
        if !canonical.starts_with(root) {
            return Err(WorkspaceError::Path(format!(
                "path resolves outside the workspace: {}",
                self.relative
            )));
        }
        Ok(canonical)
    }

    pub(super) fn into_entry(self, level: MetadataLevel) -> Result<WorkspaceEntry, WorkspaceError> {
        let needs_metadata = match (level, self.file_type) {
            (MetadataLevel::Minimal, _) | (MetadataLevel::Size, WorkspaceFileType::Directory) => {
                false
            }
            (MetadataLevel::Size | MetadataLevel::Full, WorkspaceFileType::File)
            | (MetadataLevel::Full, WorkspaceFileType::Directory) => true,
        };
        let metadata = match (self.followed_metadata, needs_metadata) {
            (Some(metadata), _) => Some(metadata),
            (None, true) => Some(self.entry.metadata().map_err(|error| {
                WorkspaceError::Path(format!(
                    "cannot inspect workspace path {}: {error}",
                    self.relative
                ))
            })?),
            (None, false) => None,
        };
        let size = if level != MetadataLevel::Minimal && self.file_type == WorkspaceFileType::File {
            metadata.as_ref().map(fs::Metadata::len)
        } else {
            None
        };
        let modified = if level == MetadataLevel::Full {
            metadata.as_ref().and_then(|value| value.modified().ok())
        } else {
            None
        };
        Ok(WorkspaceEntry {
            path: self.entry.into_path(),
            relative: self.relative,
            file_type: self.file_type,
            size,
            modified,
        })
    }
}
