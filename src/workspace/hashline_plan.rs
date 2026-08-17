use std::collections::HashSet;

use crate::workspace::hashline_apply::apply_operations;
use crate::workspace::hashline_locator::resolve_operations;
use crate::workspace::hashline_types::{
    HashlineFilePlan, HashlinePlan, HashlineSection, PreparedHashlineSection,
};
use crate::workspace::path::{normalize_relative, resolve_new_file};
use crate::workspace::workflows::load_current;
use crate::workspace::{MutationContext, Workspace, WorkspaceError, sha256};

pub(crate) fn build_plan(
    workspace: &Workspace,
    sections: &[HashlineSection],
    context: &MutationContext,
) -> Result<HashlinePlan, WorkspaceError> {
    if sections.is_empty() {
        return Err(WorkspaceError::Patch(
            "Hashline patch must contain at least one file section".to_owned(),
        ));
    }
    if sections.len() > 64 {
        return Err(WorkspaceError::Limit(
            "Hashline patch exceeds the 64-path limit".to_owned(),
        ));
    }

    let mut seen_paths = HashSet::new();
    let mut prepared = Vec::new();
    for section in sections {
        context.ensure_active()?;
        let path = normalize_relative(&section.path)?;
        if !seen_paths.insert(path.clone()) {
            return Err(WorkspaceError::Patch(format!(
                "duplicate Hashline file section: {}",
                section.path
            )));
        }
        prepared.push(prepare_section(workspace, section, &path, context)?);
    }
    validate_path_conflicts(workspace, &prepared, &seen_paths)?;

    let mut named = workspace.named_registers()?;
    let mut anonymous = None;
    for section in &mut prepared {
        apply_operations(section, &mut named, &mut anonymous)?;
    }

    Ok(HashlinePlan {
        files: prepared.into_iter().map(|section| section.file).collect(),
        named_registers: named,
    })
}

fn prepare_section(
    workspace: &Workspace,
    section: &HashlineSection,
    path: &str,
    context: &MutationContext,
) -> Result<PreparedHashlineSection, WorkspaceError> {
    let (target, current) =
        load_current(workspace, path, &context.policy).map_err(|error| match error {
            WorkspaceError::Read(_) => WorkspaceError::Patch(format!(
                "Hashline cannot edit a missing path; use write: {path}"
            )),
            other => other,
        })?;
    let authorization = workspace.observations()?.resolve_tag(path, &section.tag)?;
    let directives = section
        .operations
        .iter()
        .filter(|operation| matches!(operation.kind.as_str(), "remove" | "move"))
        .collect::<Vec<_>>();
    if directives.len() > 1 {
        return Err(WorkspaceError::Patch(format!(
            "Hashline section has incompatible file directives: {path}"
        )));
    }
    let destination = directives
        .first()
        .and_then(|operation| operation.destination.as_deref())
        .map(normalize_relative)
        .transpose()?;
    let remove = directives
        .first()
        .is_some_and(|operation| operation.kind == "remove");
    if remove && section.operations.len() > 1 {
        return Err(WorkspaceError::Patch(format!(
            "Hashline REM cannot be combined with edits: {path}"
        )));
    }
    if remove || destination.is_some() {
        require_current_complete(path, &current.source, &authorization)?;
    }

    let editable = section
        .operations
        .iter()
        .filter(|operation| !matches!(operation.kind.as_str(), "remove" | "move"))
        .cloned()
        .collect::<Vec<_>>();
    let operations = resolve_operations(path, &current, &authorization, &editable)?;

    Ok(PreparedHashlineSection {
        file: HashlineFilePlan {
            path: path.to_owned(),
            destination,
            remove,
            target,
            final_source: current.source.clone(),
            changed_range: None,
            current,
        },
        operations,
    })
}

fn require_current_complete(
    path: &str,
    source: &str,
    authorization: &crate::workspace::observation_types::Authorization,
) -> Result<(), WorkspaceError> {
    authorization.require_complete(path)?;
    if authorization.receipt.content_sha256 != sha256(source.as_bytes()) {
        return Err(WorkspaceError::Stale(format!(
            "complete Hashline observation changed; reread {path}"
        )));
    }
    Ok(())
}

fn validate_path_conflicts(
    workspace: &Workspace,
    sections: &[PreparedHashlineSection],
    sources: &HashSet<String>,
) -> Result<(), WorkspaceError> {
    let mut destinations = HashSet::new();
    for section in sections {
        let Some(destination) = section.file.destination.as_deref() else {
            continue;
        };
        let destination = normalize_relative(destination)?;
        if sources.contains(&destination) || !destinations.insert(destination.clone()) {
            return Err(WorkspaceError::Patch(format!(
                "Hashline move destination conflicts with another section: {destination}"
            )));
        }
        resolve_new_file(workspace.root(), &destination, false)?;
    }
    Ok(())
}
