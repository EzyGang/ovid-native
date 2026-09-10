use std::{fs, path::Path};

use globset::{GlobBuilder, GlobMatcher};

use crate::workspace::{WorkspaceError, path::validate_relative};

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

    let selected_path = root.join(&normalized);
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
