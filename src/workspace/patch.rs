use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use crate::workspace::content::NormalizedText;
use crate::workspace::path::{resolve_new_file, validate_relative};
use crate::workspace::workflows::{MutationContext, edit_result, load_current};
use crate::workspace::{
    EditResult, FileChange, LineRange, PostEditSource, Workspace, WorkspaceError,
    atomic_replace_path, create_file, move_file_noclobber, sha256,
};

const MAX_PATCH_OPERATIONS: usize = 256;
const MAX_PATCH_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug)]
pub(crate) struct PatchOperation {
    pub kind: PatchOperationKind,
    pub path: String,
    pub destination: Option<String>,
    pub body: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PatchOperationKind {
    Create,
    Update,
    Delete,
}

#[derive(Debug)]
enum PreparedOperation {
    Create {
        path: String,
        text: NormalizedText,
    },
    Update {
        path: String,
        target: PathBuf,
        destination: Option<(String, PathBuf)>,
        before: String,
        text: NormalizedText,
        bytes: Vec<u8>,
        changed: Option<LineRange>,
    },
    Delete {
        path: String,
        target: PathBuf,
        before: String,
    },
}

impl PreparedOperation {
    fn summary(&self) -> String {
        match self {
            Self::Create { path, .. } | Self::Update { path, .. } | Self::Delete { path, .. } => {
                path.clone()
            }
        }
    }
}

impl Workspace {
    pub(crate) fn apply_patch_operations(
        &self,
        operations: &[PatchOperation],
        context: &MutationContext,
        expected_mode: &str,
    ) -> Result<EditResult, WorkspaceError> {
        if operations.is_empty() {
            return Err(WorkspaceError::Patch(
                "workspace patch must contain at least one operation".to_owned(),
            ));
        }
        if operations.len() > MAX_PATCH_OPERATIONS {
            return Err(WorkspaceError::Limit(format!(
                "workspace patch exceeds operation limit: {} > {MAX_PATCH_OPERATIONS}",
                operations.len()
            )));
        }
        let patch_bytes = operations.iter().try_fold(0_usize, |size, operation| {
            size.checked_add(operation.path.len())
                .and_then(|size| {
                    size.checked_add(operation.destination.as_ref().map_or(0, String::len))
                })
                .and_then(|size| size.checked_add(operation.body.as_ref().map_or(0, String::len)))
        });
        if patch_bytes.is_none_or(|size| size > MAX_PATCH_BYTES) {
            return Err(WorkspaceError::Limit(format!(
                "workspace patch exceeds byte limit: {MAX_PATCH_BYTES}"
            )));
        }
        let _coordinator = self.write_guard()?;
        self.validate_mutation(context, expected_mode)?;
        let prepared = self.preflight_patch(operations, context)?;
        self.commit_patch(prepared, context)
    }

    fn preflight_patch(
        &self,
        operations: &[PatchOperation],
        context: &MutationContext,
    ) -> Result<Vec<PreparedOperation>, WorkspaceError> {
        let mut prepared = Vec::with_capacity(operations.len());
        let mut paths = HashSet::new();
        for operation in operations {
            validate_relative(&operation.path)?;
            if !paths.insert(operation.path.clone()) {
                return Err(WorkspaceError::Patch(format!(
                    "workspace patch contains duplicate source path: {}",
                    operation.path
                )));
            }
            if operation.kind != PatchOperationKind::Update && operation.destination.is_some() {
                return Err(WorkspaceError::Patch(format!(
                    "only update patches accept a destination: {}",
                    operation.path
                )));
            }
            match operation.kind {
                PatchOperationKind::Create => {
                    let body = operation.body.as_deref().ok_or_else(|| {
                        WorkspaceError::Patch(format!(
                            "create patch has no content: {}",
                            operation.path
                        ))
                    })?;
                    let text = NormalizedText::from_replacement(body);
                    let create_parents = context.policy.create_parent_directories;
                    resolve_new_file(self.root(), &operation.path, create_parents).map(drop)?;
                    prepared.push(PreparedOperation::Create {
                        path: operation.path.clone(),
                        text,
                    });
                }
                PatchOperationKind::Update => {
                    let body = operation.body.as_deref().ok_or_else(|| {
                        WorkspaceError::Patch(format!(
                            "update patch has no diff: {}",
                            operation.path
                        ))
                    })?;
                    let (target, current) = load_current(self, &operation.path, &context.policy)?;
                    let before = sha256(current.source.as_bytes());
                    let authorization = self.observations()?.current(&operation.path, &before)?;
                    if operation.destination.is_some() {
                        authorization.require_complete(&operation.path)?;
                    }
                    let (source, required, changed) = apply_unified_diff(
                        &current,
                        body,
                        context.policy.allow_fuzzy_replace,
                        context.policy.fuzzy_replace_threshold,
                    )?;
                    authorization.require_lines(&operation.path, &current, &required)?;
                    let text = NormalizedText::from_replacement(&source);
                    let bytes = current.serialize_with_current(&text.source);
                    let destination = operation
                        .destination
                        .as_deref()
                        .map(|path| {
                            validate_relative(path)?;
                            if !paths.insert(path.to_owned()) {
                                return Err(WorkspaceError::Patch(format!(
                                    "workspace patch contains a duplicate path: {path}"
                                )));
                            }
                            resolve_new_file(self.root(), path, false)
                                .map(|target| (path.to_owned(), target))
                        })
                        .transpose()?;
                    prepared.push(PreparedOperation::Update {
                        path: operation.path.clone(),
                        target,
                        destination,
                        before,
                        text,
                        bytes,
                        changed,
                    });
                }
                PatchOperationKind::Delete => {
                    if operation.body.is_some() || operation.destination.is_some() {
                        return Err(WorkspaceError::Patch(format!(
                            "delete patch cannot contain content or destination: {}",
                            operation.path
                        )));
                    }
                    let (target, current) = load_current(self, &operation.path, &context.policy)?;
                    let before = sha256(current.source.as_bytes());
                    self.observations()?
                        .current(&operation.path, &before)?
                        .require_complete(&operation.path)?;
                    prepared.push(PreparedOperation::Delete {
                        path: operation.path.clone(),
                        target,
                        before,
                    });
                }
            }
        }
        Ok(prepared)
    }

    fn commit_patch(
        &self,
        prepared: Vec<PreparedOperation>,
        context: &MutationContext,
    ) -> Result<EditResult, WorkspaceError> {
        let pending = prepared
            .iter()
            .map(PreparedOperation::summary)
            .collect::<Vec<_>>();
        let mut landed = Vec::new();
        let mut changes = Vec::with_capacity(prepared.len());
        let mut post_edit_sources = Vec::new();
        for (index, operation) in prepared.into_iter().enumerate() {
            let result = self.commit_patch_operation(
                operation,
                context,
                &mut changes,
                &mut post_edit_sources,
            );
            match result {
                Ok(summary) => landed.push(summary),
                Err(WorkspaceError::PartialCommit {
                    landed: operation_landed,
                    pending: operation_pending,
                    ..
                }) => {
                    landed.extend(operation_landed);
                    let mut remaining = operation_pending;
                    remaining.extend_from_slice(&pending[index + 1..]);
                    return Err(WorkspaceError::PartialCommit {
                        landed,
                        pending: remaining,
                        changes,
                    });
                }
                Err(error) if landed.is_empty() => return Err(error),
                Err(_) => {
                    return Err(WorkspaceError::PartialCommit {
                        landed,
                        pending: pending[index..].to_vec(),
                        changes,
                    });
                }
            }
        }

        Ok(edit_result(context, changes, post_edit_sources, None, None))
    }

    fn commit_patch_operation(
        &self,
        operation: PreparedOperation,
        context: &MutationContext,
        changes: &mut Vec<FileChange>,
        post_edit_sources: &mut Vec<PostEditSource>,
    ) -> Result<String, WorkspaceError> {
        context.ensure_active()?;

        match operation {
            PreparedOperation::Create { path, text } => {
                let create_parents = context.policy.create_parent_directories;
                context.ensure_active()?;
                create_file(self.root(), &path, &text.serialize(), create_parents)?;
                let (file_generation, revision) = self.mark_file_changed(&path)?;
                let after = sha256(text.source.as_bytes());
                let post =
                    self.record_post_edit(&path, &text, file_generation, None, &context.policy)?;
                let observation = post.as_ref().map(|source| source.observation.clone());
                post_edit_sources.extend(post);
                changes.push(FileChange {
                    path: path.clone(),
                    operation: "create".to_owned(),
                    destination: None,
                    before_sha256: None,
                    after_sha256: Some(after),
                    observation,
                    file_generation,
                    revision,
                });
                Ok(path)
            }
            PreparedOperation::Update {
                path,
                target,
                destination,
                before,
                text,
                bytes,
                changed,
            } => {
                context.ensure_active()?;
                atomic_replace_path(&target, &path, &before, &bytes)?;
                let (result_path, operation, destination_name) = match destination {
                    Some((name, destination_target)) => {
                        context.ensure_active()?;
                        move_file_noclobber(&target, &destination_target, &path, &name)?;
                        (name.clone(), "move", Some(name))
                    }
                    None => (path.clone(), "update", None),
                };
                let (file_generation, revision) = self.mark_file_changed(&result_path)?;
                let after = sha256(text.source.as_bytes());
                let post = self.record_post_edit(
                    &result_path,
                    &text,
                    file_generation,
                    changed,
                    &context.policy,
                )?;
                let observation = post.as_ref().map(|source| source.observation.clone());
                post_edit_sources.extend(post);
                changes.push(FileChange {
                    path: path.clone(),
                    operation: operation.to_owned(),
                    destination: destination_name,
                    before_sha256: Some(before),
                    after_sha256: Some(after),
                    observation,
                    file_generation,
                    revision,
                });
                Ok(path)
            }
            PreparedOperation::Delete {
                path,
                target,
                before,
            } => {
                context.ensure_active()?;
                fs::remove_file(&target).map_err(|error| {
                    WorkspaceError::Write(format!("cannot delete {path}: {error}"))
                })?;
                let (file_generation, revision) = self.mark_file_changed(&path)?;
                changes.push(FileChange {
                    path: path.clone(),
                    operation: "delete".to_owned(),
                    destination: None,
                    before_sha256: Some(before),
                    after_sha256: None,
                    observation: None,
                    file_generation,
                    revision,
                });
                Ok(path)
            }
        }
    }
}

pub(crate) fn parse_apply_patch(input: &str) -> Result<Vec<PatchOperation>, WorkspaceError> {
    if input.len() > MAX_PATCH_BYTES {
        return Err(WorkspaceError::Limit(format!(
            "workspace patch exceeds byte limit: {MAX_PATCH_BYTES}"
        )));
    }
    let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
    let lines = normalized.lines().collect::<Vec<_>>();
    if lines.first() != Some(&"*** Begin Patch") || lines.last() != Some(&"*** End Patch") {
        return Err(WorkspaceError::Patch(
            "apply-patch input must use the Begin Patch and End Patch envelope".to_owned(),
        ));
    }
    let mut operations = Vec::new();
    let mut index = 1;
    while index + 1 < lines.len() {
        let header = lines[index];
        let (kind, path) = if let Some(path) = header.strip_prefix("*** Add File: ") {
            (PatchOperationKind::Create, path)
        } else if let Some(path) = header.strip_prefix("*** Update File: ") {
            (PatchOperationKind::Update, path)
        } else if let Some(path) = header.strip_prefix("*** Delete File: ") {
            (PatchOperationKind::Delete, path)
        } else {
            return Err(WorkspaceError::Patch(
                "apply-patch input contains an invalid operation header".to_owned(),
            ));
        };
        index += 1;
        let destination = if kind == PatchOperationKind::Update {
            lines
                .get(index)
                .and_then(|line| line.strip_prefix("*** Move to: "))
                .map(|value| {
                    index += 1;
                    value.to_owned()
                })
        } else {
            None
        };
        let body_start = index;
        while index + 1 < lines.len() && !lines[index].starts_with("*** ") {
            index += 1;
        }
        let body_lines = &lines[body_start..index];
        let body = match kind {
            PatchOperationKind::Create => Some(parse_add_body(body_lines)?),
            PatchOperationKind::Update => Some(body_lines.join("\n")),
            PatchOperationKind::Delete if body_lines.is_empty() => None,
            PatchOperationKind::Delete => {
                return Err(WorkspaceError::Patch(
                    "apply-patch delete operation cannot contain a body".to_owned(),
                ));
            }
        };
        operations.push(PatchOperation {
            kind,
            path: path.to_owned(),
            destination,
            body,
        });
    }
    Ok(operations)
}

pub(crate) fn parse_structured_patch(
    path: &str,
    edits: &[(String, Option<String>, Option<String>)],
) -> Result<Vec<PatchOperation>, WorkspaceError> {
    edits
        .iter()
        .map(|(operation, diff, destination)| {
            let kind = match operation.as_str() {
                "create" => PatchOperationKind::Create,
                "update" => PatchOperationKind::Update,
                "delete" => PatchOperationKind::Delete,
                _ => {
                    return Err(WorkspaceError::Patch(
                        "structured patch contains an invalid operation".to_owned(),
                    ));
                }
            };
            let body = match kind {
                PatchOperationKind::Create => diff
                    .as_deref()
                    .map(|source| parse_add_body(&source.lines().collect::<Vec<_>>()))
                    .transpose()?,
                _ => diff.clone(),
            };
            Ok(PatchOperation {
                kind,
                path: path.to_owned(),
                destination: destination.clone(),
                body,
            })
        })
        .collect()
}

fn parse_add_body(lines: &[&str]) -> Result<String, WorkspaceError> {
    if lines.iter().any(|line| !line.starts_with('+')) {
        return Err(WorkspaceError::Patch(
            "created file content lines must begin with '+'".to_owned(),
        ));
    }
    Ok(lines
        .iter()
        .map(|line| &line[1..])
        .collect::<Vec<_>>()
        .join("\n"))
}

fn apply_unified_diff(
    current: &NormalizedText,
    diff: &str,
    allow_fuzzy: bool,
    threshold: f64,
) -> Result<(String, HashSet<usize>, Option<LineRange>), WorkspaceError> {
    let hunks = parse_hunks(diff)?;
    let source_lines = (1..=current.total_lines())
        .filter_map(|number| current.line(number).map(str::to_owned))
        .collect::<Vec<_>>();
    let mut matches = Vec::with_capacity(hunks.len());
    let mut required = HashSet::new();
    for hunk in &hunks {
        let old = hunk
            .lines
            .iter()
            .filter(|line| line.kind != '+')
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>();
        let positions = exact_positions(&source_lines, &old);
        let start = match positions.as_slice() {
            [position] => *position,
            [] if allow_fuzzy => fuzzy_position(&source_lines, &old, threshold)?,
            [] => {
                return Err(WorkspaceError::Patch(
                    "patch context was not found".to_owned(),
                ));
            }
            _ => {
                return Err(WorkspaceError::Patch(
                    "patch context is ambiguous".to_owned(),
                ));
            }
        };
        if old.is_empty() {
            let anchor = start.min(source_lines.len().saturating_sub(1)) + 1;
            required.insert(anchor);
        } else {
            required.extend((start + 1)..=(start + old.len()));
        }
        matches.push((start, old.len(), hunk));
    }
    matches.sort_by_key(|(start, _, _)| *start);
    for windows in matches.windows(2) {
        if windows[0].0 + windows[0].1 > windows[1].0 {
            return Err(WorkspaceError::Patch("patch hunks overlap".to_owned()));
        }
    }
    let changed_start = matches.first().map(|(start, _, _)| start + 1);
    let mut result = source_lines;
    for (start, old_length, hunk) in matches.into_iter().rev() {
        let replacement = hunk
            .lines
            .iter()
            .filter(|line| line.kind != '-')
            .map(|line| line.text.clone())
            .collect::<Vec<_>>();
        result.splice(start..start + old_length, replacement);
    }
    let mut source = result.join("\n");
    if current.source.ends_with('\n') {
        source.push('\n');
    }
    let changed = changed_start.map(|start| LineRange {
        start: start.saturating_sub(2).max(1),
        end: start.saturating_add(4).min(result.len()),
    });
    Ok((source, required, changed))
}

#[derive(Debug)]
struct HunkLine {
    kind: char,
    text: String,
}

#[derive(Debug)]
struct Hunk {
    lines: Vec<HunkLine>,
}

fn parse_hunks(diff: &str) -> Result<Vec<Hunk>, WorkspaceError> {
    let normalized = diff.replace("\r\n", "\n").replace('\r', "\n");
    let mut hunks = Vec::new();
    let mut current: Option<Vec<HunkLine>> = None;
    for line in normalized.lines() {
        if line.starts_with("@@") {
            if let Some(lines) = current.take() {
                validate_hunk(&lines)?;
                hunks.push(Hunk { lines });
            }
            current = Some(Vec::new());
            continue;
        }
        let lines = current.as_mut().ok_or_else(|| {
            WorkspaceError::Patch("patch diff must begin with an @@ hunk header".to_owned())
        })?;
        let kind = line
            .chars()
            .next()
            .ok_or_else(|| WorkspaceError::Patch("patch hunk contains an empty row".to_owned()))?;
        if !matches!(kind, ' ' | '+' | '-') {
            return Err(WorkspaceError::Patch(
                "patch hunk rows must begin with space, '+', or '-'".to_owned(),
            ));
        }
        lines.push(HunkLine {
            kind,
            text: line[1..].to_owned(),
        });
    }
    if let Some(lines) = current {
        validate_hunk(&lines)?;
        hunks.push(Hunk { lines });
    }
    if hunks.is_empty() {
        return Err(WorkspaceError::Patch(
            "patch diff must contain at least one hunk".to_owned(),
        ));
    }
    Ok(hunks)
}

fn validate_hunk(lines: &[HunkLine]) -> Result<(), WorkspaceError> {
    if !lines.iter().any(|line| matches!(line.kind, '+' | '-')) {
        return Err(WorkspaceError::Patch(
            "every patch hunk must contain a change".to_owned(),
        ));
    }
    Ok(())
}

fn exact_positions(source: &[String], pattern: &[&str]) -> Vec<usize> {
    if pattern.is_empty() {
        return Vec::new();
    }
    source
        .windows(pattern.len())
        .enumerate()
        .filter(|(_, candidate)| {
            candidate
                .iter()
                .zip(pattern)
                .all(|(candidate, expected)| candidate == expected)
        })
        .map(|(index, _)| index)
        .collect()
}

fn fuzzy_position(
    source: &[String],
    pattern: &[&str],
    threshold: f64,
) -> Result<usize, WorkspaceError> {
    if pattern.is_empty() || pattern.len() > source.len() {
        return Err(WorkspaceError::Patch(
            "fuzzy patch context was not found".to_owned(),
        ));
    }
    let expected = pattern.join("\n");
    let mut candidates = source
        .windows(pattern.len())
        .enumerate()
        .map(|(index, lines)| (index, similarity(&expected, &lines.join("\n"))))
        .filter(|(_, confidence)| *confidence >= threshold)
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| right.1.total_cmp(&left.1));
    match candidates.as_slice() {
        [] => Err(WorkspaceError::Patch(
            "fuzzy patch context was not found".to_owned(),
        )),
        [candidate] => Ok(candidate.0),
        [first, second, ..] if first.1 > second.1 => Ok(first.0),
        _ => Err(WorkspaceError::Patch(
            "fuzzy patch context is ambiguous".to_owned(),
        )),
    }
}

fn similarity(left: &str, right: &str) -> f64 {
    let maximum = left.chars().count().max(right.chars().count());
    if maximum == 0 {
        return 1.0;
    }
    1.0 - levenshtein(left, right) as f64 / maximum as f64
}

fn levenshtein(left: &str, right: &str) -> usize {
    let right = right.chars().collect::<Vec<_>>();
    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    for (left_index, left_character) in left.chars().enumerate() {
        let mut current = vec![left_index + 1];
        for (right_index, right_character) in right.iter().enumerate() {
            current.push(
                (previous[right_index + 1] + 1)
                    .min(current[right_index] + 1)
                    .min(previous[right_index] + usize::from(left_character != *right_character)),
            );
        }
        previous = current;
    }
    previous[right.len()]
}
