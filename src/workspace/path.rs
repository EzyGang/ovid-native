use std::fs;
use std::path::{Component, Path, PathBuf};

use globset::{GlobBuilder, GlobMatcher};

use crate::workspace::WorkspaceError;

#[derive(Clone)]
pub(crate) enum Selection {
    All,
    Exact(String),
    Directory(String),
    Glob {
        matcher: GlobMatcher,
        literal_prefix: String,
        max_depth: Option<usize>,
    },
}

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

pub(crate) fn build_selections(
    root: &Path,
    values: &[String],
) -> Result<Vec<Selection>, WorkspaceError> {
    if values.is_empty() {
        return Err(WorkspaceError::Configuration(
            "at least one workspace selection is required".to_owned(),
        ));
    }

    values
        .iter()
        .map(|value| build_selection(root, value))
        .collect()
}

pub(crate) fn selected(relative: &str, selections: &[Selection]) -> bool {
    selections.iter().any(|selection| match selection {
        Selection::All => true,
        Selection::Exact(path) => relative == path,
        Selection::Directory(path) => relative == path || is_descendant(relative, path),
        Selection::Glob { matcher, .. } => matcher.is_match(relative),
    })
}

pub(crate) fn relevant_directory(relative: &str, selections: &[Selection]) -> bool {
    if relative.is_empty() {
        return true;
    }

    selections.iter().any(|selection| match selection {
        Selection::All => true,
        Selection::Exact(path) => is_descendant(path, relative),
        Selection::Directory(path) => {
            relative == path || is_descendant(path, relative) || is_descendant(relative, path)
        }
        Selection::Glob {
            matcher,
            literal_prefix,
            max_depth,
        } => {
            if matcher.is_match(relative) {
                return true;
            }
            let within_prefix = literal_prefix.is_empty()
                || relative == literal_prefix
                || is_descendant(literal_prefix, relative)
                || is_descendant(relative, literal_prefix);
            let below_maximum_depth =
                max_depth.is_none_or(|maximum| component_count(relative) < maximum);
            within_prefix && below_maximum_depth
        }
    })
}

pub(crate) fn explicitly_selected_file(relative: &str, selections: &[Selection]) -> bool {
    selections
        .iter()
        .any(|selection| matches!(selection, Selection::Exact(path) if path == relative))
}

pub(crate) fn explicitly_includes_node_modules(values: &[String]) -> bool {
    values.iter().any(|value| {
        normalize(value)
            .split('/')
            .any(|component| component == "node_modules")
    })
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
    let target = resolve_contained_entry(root, relative)?;
    if !fs::metadata(&target).is_ok_and(|metadata| metadata.is_file()) {
        return Err(WorkspaceError::Path(format!(
            "path is not a regular file: {relative}"
        )));
    }

    Ok(target)
}

pub(crate) fn resolve_contained_directory(
    root: &Path,
    relative: &str,
) -> Result<PathBuf, WorkspaceError> {
    let target = resolve_contained_entry(root, relative)?;
    if !fs::metadata(&target).is_ok_and(|metadata| metadata.is_dir()) {
        return Err(WorkspaceError::Path(format!(
            "path is not a directory: {relative}"
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

fn build_selection(root: &Path, value: &str) -> Result<Selection, WorkspaceError> {
    validate_relative(value)?;
    let normalized = normalize(value);
    if normalized == "." {
        return Ok(Selection::All);
    }
    if value.chars().any(|character| "*?[]{}".contains(character)) {
        let glob = GlobBuilder::new(&normalized)
            .literal_separator(true)
            .backslash_escape(false)
            .build()
            .map_err(|error| WorkspaceError::Path(format!("invalid path glob {value}: {error}")))?;
        let literal_prefix = literal_prefix(&normalized);
        let max_depth = (!normalized.split('/').any(|component| component == "**"))
            .then(|| component_count(&normalized));

        return Ok(Selection::Glob {
            matcher: glob.compile_matcher(),
            literal_prefix,
            max_depth,
        });
    }

    let selected_path = root.join(value);
    let metadata = fs::symlink_metadata(&selected_path).map_err(|error| {
        WorkspaceError::Path(format!("cannot access workspace path {value}: {error}"))
    })?;
    if metadata.file_type().is_symlink()
        && fs::metadata(&selected_path).is_ok_and(|target| target.is_dir())
    {
        return Err(WorkspaceError::Path(format!(
            "directory symlinks are not followed: {value}"
        )));
    }
    if metadata.is_dir() {
        return Ok(Selection::Directory(normalized));
    }

    Ok(Selection::Exact(normalized))
}

pub(crate) fn validate_relative(value: &str) -> Result<(), WorkspaceError> {
    if value.is_empty() || value.contains('\0') {
        return Err(WorkspaceError::Path(
            "workspace paths must be non-empty and contain no NUL bytes".to_owned(),
        ));
    }

    let path = Path::new(value);
    if path.is_absolute()
        || path
            .components()
            .any(|component| component == Component::ParentDir)
    {
        return Err(WorkspaceError::Path(format!(
            "workspace path must remain relative: {value}"
        )));
    }

    Ok(())
}

fn normalize(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let trimmed = normalized.trim_start_matches("./");
    if trimmed.is_empty() {
        ".".to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn literal_prefix(pattern: &str) -> String {
    pattern
        .split('/')
        .take_while(|component| {
            !component
                .chars()
                .any(|character| "*?[]{}".contains(character))
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn component_count(path: &str) -> usize {
    path.split('/')
        .filter(|component| !component.is_empty())
        .count()
}

fn is_descendant(path: &str, ancestor: &str) -> bool {
    path.strip_prefix(ancestor)
        .is_some_and(|suffix| suffix.starts_with('/'))
}
