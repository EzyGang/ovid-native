use std::fs;

use crate::workspace::patch::{PatchOperation, PatchOperationKind};
use crate::workspace::{
    Cancellation, LineRange, MutationContext, Workspace, WorkspaceError, WorkspacePolicy,
    parse_apply_patch, parse_structured_patch,
};

fn context(workspace: &Workspace, mode: &str) -> MutationContext {
    let mode = workspace.set_edit_mode(mode).expect("edit mode");
    let policy = workspace.policy().expect("policy");
    MutationContext {
        mode: mode.mode,
        mode_generation: mode.generation,
        policy_generation: policy.generation,
        policy: policy.policy,
        cancellation: Cancellation::new(),
    }
}

#[test]
fn workspace_reads_normalized_source_and_guards_whole_file_writes() {
    let root = tempfile::tempdir().expect("workspace");
    fs::write(
        root.path().join("source.txt"),
        b"\xef\xbb\xbfone\r\ntwo\r\n",
    )
    .expect("source");
    let workspace = Workspace::new(&root.path().to_string_lossy()).expect("workspace");

    let partial = workspace
        .read_file("source.txt", &[LineRange { start: 1, end: 1 }])
        .expect("partial read");
    assert_eq!(partial.lines.len(), 1);
    assert_eq!(partial.lines[0].text, "one");
    assert!(!partial.complete_presentation);
    let partial_tag = partial.observation.expect("partial observation").tag;
    let replacement = workspace.replace_text_file(
        "source.txt",
        "replacement",
        &partial_tag,
        &MutationContext::write(workspace.policy().expect("policy"), Cancellation::new()),
    );
    assert!(matches!(replacement, Err(WorkspaceError::UnseenLine(_))));

    let complete = workspace
        .read_file("source.txt", &[])
        .expect("complete read");
    let tag = complete.observation.expect("complete observation").tag;
    let result = workspace
        .replace_text_file(
            "source.txt",
            "replacement\n",
            &tag,
            &MutationContext::write(workspace.policy().expect("policy"), Cancellation::new()),
        )
        .expect("replace");
    assert_eq!(
        fs::read(root.path().join("source.txt")).expect("source"),
        b"replacement\n"
    );
    assert_eq!(result.changes[0].operation, "update");
    assert_eq!(result.post_edit_sources[0].lines[0].text, "replacement");

    workspace
        .create_text_file(
            "created.txt",
            "created",
            false,
            &MutationContext::write(workspace.policy().expect("policy"), Cancellation::new()),
        )
        .expect("create");
    let duplicate = workspace.create_text_file(
        "created.txt",
        "duplicate",
        false,
        &MutationContext::write(workspace.policy().expect("policy"), Cancellation::new()),
    );
    assert!(matches!(duplicate, Err(WorkspaceError::Write(_))));
}

#[test]
fn replace_requires_current_seen_lines_and_respects_fuzzy_policy() {
    let root = tempfile::tempdir().expect("workspace");
    fs::write(root.path().join("source.txt"), "alpha\nbeta\ngamma\n").expect("source");
    let workspace = Workspace::new(&root.path().to_string_lossy()).expect("workspace");
    workspace
        .read_file("source.txt", &[LineRange { start: 1, end: 1 }])
        .expect("partial read");

    let unseen = workspace.replace_text(
        "source.txt",
        "beta",
        "changed",
        false,
        &context(&workspace, "replace"),
    );
    assert!(matches!(unseen, Err(WorkspaceError::UnseenLine(_))));

    workspace
        .read_file("source.txt", &[])
        .expect("complete read");
    let missing = workspace.replace_text(
        "source.txt",
        "betx",
        "changed",
        false,
        &context(&workspace, "replace"),
    );
    assert!(matches!(missing, Err(WorkspaceError::Patch(_))));

    let policy = WorkspacePolicy {
        allow_fuzzy_replace: true,
        fuzzy_replace_threshold: 0.7,
        ..WorkspacePolicy::default()
    };
    workspace.set_policy(policy).expect("fuzzy policy");
    let fuzzy = workspace
        .replace_text(
            "source.txt",
            "betx",
            "changed",
            false,
            &context(&workspace, "replace"),
        )
        .expect("fuzzy replace");
    assert_eq!(fuzzy.matching_strategy.as_deref(), Some("fuzzy"));
    assert_eq!(
        fs::read_to_string(root.path().join("source.txt")).expect("source"),
        "alpha\nchanged\ngamma\n"
    );
}

#[test]
fn mutations_reject_stale_mode_and_policy_contexts() {
    let root = tempfile::tempdir().expect("workspace");
    fs::write(root.path().join("source.txt"), "source\n").expect("source");
    let workspace = Workspace::new(&root.path().to_string_lossy()).expect("workspace");
    let stale_mode = context(&workspace, "replace");
    workspace.set_edit_mode("patch").expect("mode change");

    assert!(matches!(
        workspace.replace_text("source.txt", "source", "changed", false, &stale_mode),
        Err(WorkspaceError::EditMode(_))
    ));

    let stale_policy = context(&workspace, "replace");
    workspace
        .set_policy(WorkspacePolicy {
            allow_fuzzy_replace: true,
            ..WorkspacePolicy::default()
        })
        .expect("policy change");

    assert!(matches!(
        workspace.replace_text("source.txt", "source", "changed", false, &stale_policy),
        Err(WorkspaceError::Stale(_))
    ));
}

#[test]
fn cancelled_mutation_does_not_create_a_file() {
    let root = tempfile::tempdir().expect("workspace");
    let workspace = Workspace::new(&root.path().to_string_lossy()).expect("workspace");
    let mutation = context(&workspace, "apply_patch");
    mutation.cancellation.cancel();

    assert!(matches!(
        workspace.create_text_file("cancelled.txt", "content", false, &mutation),
        Err(WorkspaceError::Cancelled)
    ));
    assert!(!root.path().join("cancelled.txt").exists());
}

#[test]
fn structured_patch_preflights_every_operation_before_commit() {
    let root = tempfile::tempdir().expect("workspace");
    fs::write(root.path().join("source.txt"), "one\ntwo\n").expect("source");
    fs::write(root.path().join("existing.txt"), "existing\n").expect("existing");
    let workspace = Workspace::new(&root.path().to_string_lossy()).expect("workspace");
    workspace
        .read_file("source.txt", &[])
        .expect("source observation");
    let mutation = context(&workspace, "patch");
    let invalid = parse_apply_patch(
        "*** Begin Patch\n*** Update File: source.txt\n@@\n-one\n+changed\n*** Add File: existing.txt\n+duplicate\n*** End Patch",
    )
    .expect("parsed patch");

    let failure = workspace.apply_patch_operations(&invalid, &mutation, "patch");
    assert!(matches!(failure, Err(WorkspaceError::Write(_))));
    assert_eq!(
        fs::read_to_string(root.path().join("source.txt")).expect("source"),
        "one\ntwo\n"
    );

    let valid = parse_structured_patch(
        "source.txt",
        &[(
            "update".to_owned(),
            Some("@@\n-one\n+changed".to_owned()),
            Some("moved.txt".to_owned()),
        )],
    )
    .expect("structured patch");
    let result = workspace
        .apply_patch_operations(&valid, &mutation, "patch")
        .expect("move patch");
    assert_eq!(result.changes[0].operation, "move");
    assert!(!root.path().join("source.txt").exists());
    assert_eq!(
        fs::read_to_string(root.path().join("moved.txt")).expect("moved"),
        "changed\ntwo\n"
    );
}

#[test]
fn apply_patch_preserves_authored_order_and_requires_valid_envelope() {
    let root = tempfile::tempdir().expect("workspace");
    let workspace = Workspace::new(&root.path().to_string_lossy()).expect("workspace");
    let operations = parse_apply_patch(
        "*** Begin Patch\n*** Add File: first.txt\n+first\n*** Add File: second.txt\n+second\n*** End Patch",
    )
    .expect("apply patch");
    let result = workspace
        .apply_patch_operations(
            &operations,
            &context(&workspace, "apply_patch"),
            "apply_patch",
        )
        .expect("apply operations");

    assert_eq!(result.changes[0].path, "first.txt");
    assert_eq!(result.changes[1].path, "second.txt");
    assert!(result.preflight_complete);
    assert!(result.commit_complete);
    assert!(matches!(
        parse_apply_patch("invalid"),
        Err(WorkspaceError::Patch(_))
    ));
}

#[test]
fn patch_limits_and_destination_conflicts_fail_before_commit() {
    let root = tempfile::tempdir().expect("workspace");
    fs::write(root.path().join("source.txt"), "one\n").expect("source");
    let workspace = Workspace::new(&root.path().to_string_lossy()).expect("workspace");
    workspace
        .read_file("source.txt", &[])
        .expect("source observation");
    let mutation = context(&workspace, "apply_patch");

    let too_many = (0..257)
        .map(|index| PatchOperation {
            kind: PatchOperationKind::Create,
            path: format!("{index}.txt"),
            destination: None,
            body: Some("content".to_owned()),
        })
        .collect::<Vec<_>>();
    assert!(matches!(
        workspace.apply_patch_operations(&too_many, &mutation, "apply_patch"),
        Err(WorkspaceError::Limit(_))
    ));
    let too_large = [PatchOperation {
        kind: PatchOperationKind::Create,
        path: "large.txt".to_owned(),
        destination: None,
        body: Some("x".repeat(4 * 1024 * 1024 + 1)),
    }];
    assert!(matches!(
        workspace.apply_patch_operations(&too_large, &mutation, "apply_patch"),
        Err(WorkspaceError::Limit(_))
    ));

    let conflicting = parse_apply_patch(
        "*** Begin Patch\n*** Add File: moved.txt\n+created\n*** Update File: source.txt\n*** Move to: moved.txt\n@@\n-one\n+changed\n*** End Patch",
    )
    .expect("conflicting patch");
    assert!(matches!(
        workspace.apply_patch_operations(&conflicting, &mutation, "apply_patch"),
        Err(WorkspaceError::Patch(_))
    ));
    assert!(!root.path().join("moved.txt").exists());
    assert_eq!(
        fs::read_to_string(root.path().join("source.txt")).expect("source"),
        "one\n"
    );

    let invalid_destination = parse_structured_patch(
        "created.txt",
        &[(
            "create".to_owned(),
            Some("+created".to_owned()),
            Some("ignored.txt".to_owned()),
        )],
    )
    .expect("structured patch");
    assert!(matches!(
        workspace.apply_patch_operations(&invalid_destination, &mutation, "apply_patch"),
        Err(WorkspaceError::Patch(_))
    ));
}
