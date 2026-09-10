use std::{fs, path::Path};

use crate::workspace::{
    Workspace, WorkspaceDirectoryEntry, WorkspaceDirectoryRead, WorkspaceDirectoryTreeRead,
    WorkspaceError, directory_tree, path,
};

impl Workspace {
    pub(crate) fn list_directory(
        &self,
        path: &str,
        depth: usize,
    ) -> Result<WorkspaceDirectoryRead, WorkspaceError> {
        let path = path::normalize_relative_directory(path)?;
        self.ensure_open()?;
        if !(1..=2).contains(&depth) {
            return Err(WorkspaceError::Limit(
                "workspace directory depth must be between one and two".to_owned(),
            ));
        }
        let target = path::resolve_contained_directory(self.root(), &path)?;
        let mut entries = Vec::new();
        let mut truncated = false;
        collect_directory_entries(
            self.root(),
            &target,
            depth,
            4096,
            &mut entries,
            &mut truncated,
        )?;
        entries.sort_by(|left, right| left.path.cmp(&right.path));

        Ok(WorkspaceDirectoryRead {
            path,
            entries,
            truncated,
        })
    }

    pub(crate) fn read_directory_tree(
        &self,
        path: &str,
        depth: usize,
        child_limit: usize,
    ) -> Result<WorkspaceDirectoryTreeRead, WorkspaceError> {
        if !(1..=4096).contains(&child_limit) {
            return Err(WorkspaceError::Limit(
                "workspace directory child limit must be between one and 4096".to_owned(),
            ));
        }
        let listing = self.list_directory(path, depth)?;
        Ok(directory_tree::build_directory_tree(listing, child_limit))
    }
}

fn collect_directory_entries(
    root: &Path,
    directory: &Path,
    depth: usize,
    limit: usize,
    entries: &mut Vec<WorkspaceDirectoryEntry>,
    truncated: &mut bool,
) -> Result<(), WorkspaceError> {
    let reader = fs::read_dir(directory).map_err(|error| {
        WorkspaceError::Read(format!("cannot list workspace directory: {error}"))
    })?;
    let mut children = reader.collect::<Result<Vec<_>, _>>().map_err(|error| {
        WorkspaceError::Read(format!("cannot list workspace directory: {error}"))
    })?;
    children.sort_by_key(std::fs::DirEntry::file_name);
    for child in children {
        if entries.len() == limit {
            *truncated = true;
            return Ok(());
        }
        let target = child.path();
        let metadata = fs::symlink_metadata(&target).map_err(|error| {
            WorkspaceError::Read(format!("cannot inspect workspace entry: {error}"))
        })?;
        let relative = path::relative_path(root, &target)?;
        let file_type = metadata.file_type();
        let kind = if file_type.is_symlink() {
            "symlink"
        } else if metadata.is_dir() {
            "directory"
        } else {
            "file"
        };
        entries.push(WorkspaceDirectoryEntry {
            path: relative,
            kind: kind.to_owned(),
            size: metadata.is_file().then_some(metadata.len()),
        });
        if depth > 1 && metadata.is_dir() {
            collect_directory_entries(root, &target, depth - 1, limit, entries, truncated)?;
            if *truncated {
                return Ok(());
            }
        }
    }
    Ok(())
}
