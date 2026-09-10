use crate::workspace::path::normalize_relative;
use crate::workspace::{EditResult, MutationContext, Workspace, WorkspaceError};

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
impl PatchOperation {
    fn normalized(&self) -> Result<Self, WorkspaceError> {
        Ok(Self {
            kind: self.kind,
            path: normalize_relative(&self.path)?,
            destination: self
                .destination
                .as_deref()
                .map(normalize_relative)
                .transpose()?,
            body: self.body.clone(),
        })
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
        let operations = operations
            .iter()
            .map(PatchOperation::normalized)
            .collect::<Result<Vec<_>, _>>()?;
        let _coordinator = self.write_guard()?;
        self.validate_mutation(context, expected_mode)?;
        let prepared = self.preflight_patch(&operations, context)?;
        self.commit_patch(prepared, context)
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
