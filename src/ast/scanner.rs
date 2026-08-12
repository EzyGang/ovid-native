use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use globset::{GlobBuilder, GlobMatcher};
use ignore::{DirEntry, WalkBuilder};

use crate::ast::AstError;
use crate::ast::types::ScanOptions;

pub struct Candidate {
    pub path: PathBuf,
    pub relative: String,
}

enum Selection {
    All,
    Exact(String),
    Directory(String),
    Glob(GlobMatcher),
}

pub fn discover(
    root: &Path,
    options: &ScanOptions,
    max_files: usize,
) -> Result<Vec<Candidate>, AstError> {
    let selections = build_selections(root, &options.paths)?;
    let mut builder = WalkBuilder::new(root);
    builder
        .follow_links(false)
        .hidden(!options.include_hidden)
        .ignore(options.respect_gitignore)
        .git_ignore(options.respect_gitignore)
        .git_exclude(options.respect_gitignore)
        .git_global(false)
        .parents(false)
        .sort_by_file_path(|left, right| left.cmp(right));

    let include_node_modules = options.include_node_modules;
    builder.filter_entry(move |entry| allowed_entry(entry, include_node_modules));

    let mut candidates = Vec::new();
    let mut canonical_paths = HashSet::new();
    for entry in builder.build() {
        let entry =
            entry.map_err(|error| AstError::Path(format!("workspace scan failed: {error}")))?;
        let relative = relative_path(root, entry.path())?;
        if relative.is_empty() || !selected(&relative, &selections) {
            continue;
        }
        let canonical = match entry.path().canonicalize() {
            Ok(path) => path,
            Err(error) => {
                return Err(AstError::Path(format!(
                    "cannot resolve workspace path {relative}: {error}"
                )));
            }
        };
        if !canonical.starts_with(root) {
            return Err(AstError::Path(format!(
                "path resolves outside the workspace: {relative}"
            )));
        }
        if !fs::metadata(&canonical).is_ok_and(|metadata| metadata.is_file()) {
            continue;
        }
        if !canonical_paths.insert(canonical) {
            continue;
        }
        if candidates.len() == max_files {
            return Err(AstError::Limit(format!(
                "workspace selection exceeds the {max_files} file limit"
            )));
        }
        candidates.push(Candidate {
            path: entry.into_path(),
            relative,
        });
    }
    candidates.sort_by(|left, right| left.relative.cmp(&right.relative));
    Ok(candidates)
}

fn build_selections(root: &Path, paths: &[String]) -> Result<Vec<Selection>, AstError> {
    if paths.is_empty() {
        return Err(AstError::Configuration(
            "at least one scan path is required".to_owned(),
        ));
    }
    paths
        .iter()
        .map(|value| build_selection(root, value))
        .collect()
}

fn build_selection(root: &Path, value: &str) -> Result<Selection, AstError> {
    if value.is_empty() || value.contains('\0') {
        return Err(AstError::Path(
            "scan paths must be non-empty and contain no NUL bytes".to_owned(),
        ));
    }
    let path = Path::new(value);
    if path.is_absolute()
        || path
            .components()
            .any(|component| component == Component::ParentDir)
    {
        return Err(AstError::Path(format!(
            "scan path must remain relative to the workspace: {value}"
        )));
    }
    let normalized = normalize(value);
    if normalized == "." {
        return Ok(Selection::All);
    }
    if value.chars().any(|character| "*?[]{}".contains(character)) {
        let glob = GlobBuilder::new(&normalized)
            .literal_separator(true)
            .backslash_escape(false)
            .build()
            .map_err(|error| AstError::Path(format!("invalid path glob {value}: {error}")))?;
        return Ok(Selection::Glob(glob.compile_matcher()));
    }

    let selected_path = root.join(path);
    let metadata = fs::symlink_metadata(&selected_path)
        .map_err(|error| AstError::Path(format!("cannot access scan path {value}: {error}")))?;
    if metadata.file_type().is_symlink()
        && fs::metadata(&selected_path).is_ok_and(|target| target.is_dir())
    {
        return Err(AstError::Path(format!(
            "directory symlinks are not followed: {value}"
        )));
    }
    if metadata.is_dir() {
        Ok(Selection::Directory(normalized))
    } else {
        Ok(Selection::Exact(normalized))
    }
}

fn selected(relative: &str, selections: &[Selection]) -> bool {
    selections.iter().any(|selection| match selection {
        Selection::All => true,
        Selection::Exact(path) => relative == path,
        Selection::Directory(path) => relative == path || relative.starts_with(&format!("{path}/")),
        Selection::Glob(glob) => glob.is_match(relative),
    })
}

fn allowed_entry(entry: &DirEntry, include_node_modules: bool) -> bool {
    let name = entry.file_name().to_string_lossy();
    name != ".git" && (include_node_modules || name != "node_modules")
}

fn relative_path(root: &Path, path: &Path) -> Result<String, AstError> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| AstError::Path("workspace scanner returned an external path".to_owned()))?;
    Ok(relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/"))
}

fn normalize(path: &str) -> String {
    path.replace('\\', "/").trim_start_matches("./").to_owned()
}
