use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::workspace::WorkspaceError;

pub(crate) fn canonical_root(value: &str) -> Result<PathBuf, WorkspaceError> {
    if value.contains('\0') {
        return Err(WorkspaceError::Configuration(
            "workspace root contains a NUL byte".to_owned(),
        ));
    }

    let root = Path::new(value).canonicalize().map_err(|error| {
        WorkspaceError::Configuration(format!("cannot resolve workspace root: {error}"))
    })?;
    if !root.is_dir() {
        return Err(WorkspaceError::Configuration(
            "workspace root must be a directory".to_owned(),
        ));
    }

    Ok(root)
}

pub(crate) fn relative_path(root: &Path, path: &Path) -> Result<String, WorkspaceError> {
    let relative = path.strip_prefix(root).map_err(|_| {
        WorkspaceError::Path("workspace scanner returned an external path".to_owned())
    })?;

    Ok(relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/"))
}

pub(crate) fn resolve_contained_file(
    root: &Path,
    relative: &str,
) -> Result<PathBuf, WorkspaceError> {
    resolve_contained_type(root, relative, fs::Metadata::is_file, "regular file")
}

pub(crate) fn resolve_contained_directory(
    root: &Path,
    relative: &str,
) -> Result<PathBuf, WorkspaceError> {
    resolve_contained_type(root, relative, fs::Metadata::is_dir, "directory")
}

fn resolve_contained_type(
    root: &Path,
    relative: &str,
    matches: fn(&fs::Metadata) -> bool,
    kind: &str,
) -> Result<PathBuf, WorkspaceError> {
    let target = resolve_contained_entry(root, relative)?;
    if !fs::metadata(&target).is_ok_and(|metadata| matches(&metadata)) {
        return Err(WorkspaceError::Path(format!(
            "path is not a {kind}: {relative}"
        )));
    }
    Ok(target)
}

pub(crate) fn resolve_contained_entry(
    root: &Path,
    relative: &str,
) -> Result<PathBuf, WorkspaceError> {
    validate_relative(relative)?;
    let joined = root.join(relative);
    reject_symlink_components(root, &joined, relative)?;
    let target = joined
        .canonicalize()
        .map_err(|error| WorkspaceError::Path(format!("cannot resolve {relative}: {error}")))?;
    if !target.starts_with(root) {
        return Err(WorkspaceError::Path(format!(
            "path resolves outside the workspace: {relative}"
        )));
    }

    Ok(target)
}

pub(crate) fn resolve_new_file(
    root: &Path,
    relative: &str,
    create_parents: bool,
) -> Result<PathBuf, WorkspaceError> {
    validate_relative(relative)?;
    let target = root.join(relative);
    if fs::symlink_metadata(&target).is_ok() {
        return Err(WorkspaceError::Write(format!(
            "workspace path already exists: {relative}"
        )));
    }
    let parent = target
        .parent()
        .ok_or_else(|| WorkspaceError::Path(format!("path has no parent: {relative}")))?;
    validate_new_parent(root, parent, relative, create_parents)?;
    Ok(target)
}
fn reject_symlink_components(
    root: &Path,
    target: &Path,
    relative: &str,
) -> Result<(), WorkspaceError> {
    let mut current = root.to_path_buf();
    for component in target
        .strip_prefix(root)
        .map_err(|_| {
            WorkspaceError::Path(format!("path resolves outside the workspace: {relative}"))
        })?
        .components()
    {
        current.push(component);
        let metadata = fs::symlink_metadata(&current)
            .map_err(|error| WorkspaceError::Path(format!("cannot inspect {relative}: {error}")))?;
        if metadata.file_type().is_symlink() {
            return Err(WorkspaceError::Path(format!(
                "workspace paths cannot traverse symlinks: {relative}"
            )));
        }
    }

    Ok(())
}

fn validate_new_parent(
    root: &Path,
    parent: &Path,
    relative: &str,
    create_parents: bool,
) -> Result<(), WorkspaceError> {
    let mut current = root.to_path_buf();
    for component in parent
        .strip_prefix(root)
        .map_err(|_| {
            WorkspaceError::Path(format!("path resolves outside the workspace: {relative}"))
        })?
        .components()
    {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(WorkspaceError::Path(format!(
                    "workspace paths cannot traverse symlinks: {relative}"
                )));
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(WorkspaceError::Path(format!(
                    "workspace parent is not a directory: {relative}"
                )));
            }
            Ok(_) => (),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && create_parents => {
                return Ok(());
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(WorkspaceError::Path(format!(
                    "workspace parent does not exist: {relative}"
                )));
            }
            Err(error) => {
                return Err(WorkspaceError::Path(format!(
                    "cannot inspect workspace parent {relative}: {error}"
                )));
            }
        }
    }

    Ok(())
}

pub(crate) fn validate_relative(value: &str) -> Result<(), WorkspaceError> {
    let separators = value.replace('\\', "/");
    if separators.is_empty() || separators.contains('\0') {
        return Err(WorkspaceError::Path(
            "workspace paths must be non-empty and contain no NUL bytes".to_owned(),
        ));
    }

    let path = Path::new(&separators);
    let bytes = separators.as_bytes();
    let windows_prefix = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    if path.is_absolute()
        || windows_prefix
        || separators.starts_with("//")
        || path
            .components()
            .any(|component| matches!(component, Component::Prefix(_) | Component::ParentDir))
    {
        return Err(WorkspaceError::Path(format!(
            "workspace path must remain relative: {value}"
        )));
    }

    Ok(())
}

pub(crate) fn normalize_relative(value: &str) -> Result<String, WorkspaceError> {
    let normalized = normalized_components(value)?;
    if normalized.is_empty() {
        return Err(WorkspaceError::Path(format!(
            "workspace paths must identify a file: {value}"
        )));
    }

    Ok(normalized.join("/"))
}

pub(crate) fn normalize_relative_directory(value: &str) -> Result<String, WorkspaceError> {
    let normalized = normalized_components(value)?;
    Ok(if normalized.is_empty() {
        ".".to_owned()
    } else {
        normalized.join("/")
    })
}

fn normalized_components(value: &str) -> Result<Vec<String>, WorkspaceError> {
    let separators = value.replace('\\', "/");
    validate_relative(&separators)?;
    let mut normalized = Vec::new();
    for component in Path::new(&separators).components() {
        match component {
            Component::CurDir => (),
            Component::Normal(component) => normalized.push(
                component
                    .to_str()
                    .ok_or_else(|| {
                        WorkspaceError::Path(format!("workspace path is not valid UTF-8: {value}"))
                    })?
                    .to_owned(),
            ),
            Component::Prefix(_) | Component::RootDir | Component::ParentDir => {
                return Err(WorkspaceError::Path(format!(
                    "workspace path must remain relative: {value}"
                )));
            }
        }
    }
    Ok(normalized)
}
