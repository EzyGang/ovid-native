use std::collections::{HashMap, HashSet};
use std::ops::Range as ByteRange;
use std::path::Path;

use ast_grep_core::Pattern;
use ast_grep_language::{LanguageExt, SupportLang};

use crate::ast::language::{canonical_name, resolve_language, strictness};
use crate::ast::search::{classify_files, discover_files, parse_issue, read_source};
use crate::ast::types::{
    FileChange, FileComputation, PreviewResult, RewriteComputation, RewriteRequest,
};
use crate::ast::{AstError, source_range};
use crate::workspace::{WorkspaceEntry, sha256};

struct Edit {
    range: ByteRange<usize>,
    replacement: String,
    sequence: usize,
}

pub fn preview(root: &Path, request: RewriteRequest) -> Result<PreviewResult, AstError> {
    validate_operations(&request.operations)?;
    let explicit = request
        .language
        .as_deref()
        .map(resolve_language)
        .transpose()?;
    let strictness = strictness(&request.strictness)?;
    let selected = discover_files(
        root,
        &request.scan,
        request.limits.max_files,
        &request.cancellation,
    )?;
    let (files, _) = classify_files(selected, explicit);
    let patterns = compile_patterns(&files, &request.operations, strictness)?;
    let mut computation_files = Vec::new();
    let mut changes = Vec::new();
    let mut issues = Vec::new();
    let mut files_searched = 0;
    let mut total_replacements = 0;

    for (candidate, language) in files {
        if request.cancellation.is_cancelled() {
            return Err(AstError::Cancelled);
        }
        let source = match read_source(
            &candidate,
            request.limits.max_file_bytes,
            &request.cancellation,
        ) {
            Ok(source) => source,
            Err(issue) => {
                if request.cancellation.is_cancelled() {
                    return Err(AstError::Cancelled);
                }
                issues.push(issue);
                continue;
            }
        };
        let tree = language.ast_grep(&source);
        if tree
            .root()
            .dfs()
            .any(|node| node.is_error() || node.is_missing())
        {
            issues.push(parse_issue(&candidate, language));
            continue;
        }
        files_searched += 1;
        let mut edits = Vec::new();
        for (sequence, (pattern, replacement)) in patterns[&language].iter().enumerate() {
            for matched in tree.root().find_all(pattern) {
                if request.cancellation.is_cancelled() {
                    return Err(AstError::Cancelled);
                }
                let edit = matched.replace_by(replacement.as_str());
                let replacement = String::from_utf8(edit.inserted_text).map_err(|_| {
                    AstError::Pattern("replacement produced invalid UTF-8".to_owned())
                })?;
                edits.push(Edit {
                    range: edit.position..edit.position + edit.deleted_length,
                    replacement,
                    sequence,
                });
            }
        }
        let edits = normalize_edits(&candidate.relative, edits)?;
        if edits.is_empty() {
            continue;
        }
        total_replacements += edits.len();
        if total_replacements > request.limits.max_replacements {
            return Err(AstError::Limit(format!(
                "rewrite exceeds the {} replacement limit",
                request.limits.max_replacements
            )));
        }
        if computation_files.len() == request.limits.max_changed_files {
            return Err(AstError::Limit(format!(
                "rewrite exceeds the {} changed-file limit",
                request.limits.max_changed_files
            )));
        }
        let updated = apply_edits(&source, &edits);
        changes.extend(edits.iter().map(|edit| {
            (
                candidate.relative.clone(),
                canonical_name(language).to_owned(),
                source[edit.range.clone()].to_owned(),
                edit.replacement.clone(),
                source_range(&source, edit.range.clone()),
            )
        }));
        computation_files.push(FileComputation {
            path: candidate.relative,
            original_sha256: sha256(source.as_bytes()),
            updated_sha256: sha256(updated.as_bytes()),
            updated,
            replacements: edits.len(),
        });
    }

    let files = computation_files.iter().map(file_change).collect();
    let computation = RewriteComputation {
        root: root.to_path_buf(),
        files: computation_files,
        total_replacements,
    };
    Ok((
        crate::ast::types::NativeAstRewriteComputation::new(computation),
        changes,
        files,
        total_replacements,
        files_searched,
        issues,
    ))
}

fn validate_operations(operations: &[(String, String)]) -> Result<(), AstError> {
    if operations.is_empty() {
        return Err(AstError::Configuration(
            "at least one rewrite operation is required".to_owned(),
        ));
    }
    if operations.iter().any(|(pattern, replacement)| {
        pattern.is_empty() || pattern.contains('\0') || replacement.contains('\0')
    }) {
        return Err(AstError::Pattern(
            "rewrite patterns must be non-empty and operations must contain no NUL bytes"
                .to_owned(),
        ));
    }
    Ok(())
}

fn compile_patterns(
    files: &[(WorkspaceEntry, SupportLang)],
    operations: &[(String, String)],
    strictness: ast_grep_core::MatchStrictness,
) -> Result<HashMap<SupportLang, Vec<(Pattern, String)>>, AstError> {
    let mut languages = HashSet::new();
    let mut patterns = HashMap::new();
    for (_, language) in files {
        if !languages.insert(*language) {
            continue;
        }
        let compiled = operations
            .iter()
            .map(|(pattern, replacement)| {
                Pattern::try_new(pattern, *language)
                    .map(|pattern| {
                        (
                            pattern.with_strictness(strictness.clone()),
                            replacement.clone(),
                        )
                    })
                    .map_err(|error| AstError::Pattern(error.to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        patterns.insert(*language, compiled);
    }
    Ok(patterns)
}

fn normalize_edits(path: &str, mut edits: Vec<Edit>) -> Result<Vec<Edit>, AstError> {
    edits.sort_by(|left, right| {
        left.range
            .start
            .cmp(&right.range.start)
            .then(left.range.end.cmp(&right.range.end))
            .then(left.sequence.cmp(&right.sequence))
    });
    edits
        .dedup_by(|right, left| left.range == right.range && left.replacement == right.replacement);
    for pair in edits.windows(2) {
        if pair[1].range.start < pair[0].range.end {
            return Err(AstError::Pattern(format!(
                "rewrite operations produce overlapping edits in {path}"
            )));
        }
    }
    Ok(edits)
}

fn apply_edits(source: &str, edits: &[Edit]) -> String {
    let additional: usize = edits
        .iter()
        .map(|edit| edit.replacement.len().saturating_sub(edit.range.len()))
        .sum();
    let mut updated = String::with_capacity(source.len() + additional);
    updated.push_str(source);
    for edit in edits.iter().rev() {
        updated.replace_range(edit.range.clone(), &edit.replacement);
    }
    updated
}

fn file_change(file: &FileComputation) -> FileChange {
    (
        file.path.clone(),
        file.original_sha256.clone(),
        file.updated_sha256.clone(),
        file.replacements,
    )
}
