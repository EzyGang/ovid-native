use std::fs;

use crate::workspace::hashline_types::{HashlineOperation, HashlineSection};
use crate::workspace::line_hash::short_line_hash;
use crate::workspace::{
    Cancellation, LineRange, MutationContext, Workspace, WorkspaceError, ensure_current_file,
    file_identity, sha256,
};

fn context(workspace: &Workspace) -> MutationContext {
    let mode = workspace.set_edit_mode("hashline").expect("edit mode");
    let policy = workspace.policy().expect("policy");
    MutationContext {
        mode: mode.mode,
        mode_generation: mode.generation,
        policy_generation: policy.generation,
        policy: policy.policy,
        cancellation: Cancellation::new(),
    }
}

fn put_range(
    start: usize,
    start_hash: &str,
    end: usize,
    end_hash: &str,
    body: &[&str],
) -> HashlineOperation {
    HashlineOperation {
        kind: "put_range".to_owned(),
        start: Some(start),
        start_hash: Some(start_hash.to_owned()),
        end: Some(end),
        end_hash: Some(end_hash.to_owned()),
        body: body.iter().map(|line| (*line).to_owned()).collect(),
        register: None,
        destination: None,
    }
}

fn cut_range(
    start: usize,
    start_hash: &str,
    end: usize,
    end_hash: &str,
    register: &str,
) -> HashlineOperation {
    HashlineOperation {
        kind: "cut_range".to_owned(),
        start: Some(start),
        start_hash: Some(start_hash.to_owned()),
        end: Some(end),
        end_hash: Some(end_hash.to_owned()),
        body: Vec::new(),
        register: Some(register.to_owned()),
        destination: None,
    }
}

fn put_end(hash: &str, register: &str) -> HashlineOperation {
    HashlineOperation {
        kind: "put_end".to_owned(),
        start: Some(1),
        start_hash: Some(hash.to_owned()),
        end: None,
        end_hash: None,
        body: Vec::new(),
        register: Some(register.to_owned()),
        destination: None,
    }
}

#[test]
fn hashline_replaces_exact_observed_lines_and_returns_fresh_source() {
    let root = tempfile::tempdir().expect("workspace");
    fs::write(root.path().join("source.txt"), "one\ntwo\nthree\n").expect("source");
    let workspace = Workspace::new(&root.path().to_string_lossy()).expect("workspace");
    let read = workspace.read_file("source.txt", &[]).expect("observation");
    let receipt = read.observation.expect("receipt");
    let operation = put_range(
        2,
        &read.lines[1].short_hash,
        2,
        &read.lines[1].short_hash,
        &["changed"],
    );

    let result = workspace
        .apply_hashline(
            &[HashlineSection {
                path: "source.txt".to_owned(),
                tag: receipt.tag.clone(),
                operations: vec![operation],
            }],
            &context(&workspace),
        )
        .expect("hashline");

    assert_eq!(
        fs::read_to_string(root.path().join("source.txt")).expect("source"),
        "one\nchanged\nthree\n"
    );
    assert_eq!(result.changes[0].operation, "update");
    assert_eq!(result.post_edit_sources[0].lines[0].number, 2);
    assert_eq!(result.post_edit_sources[0].lines[0].text, "changed");
    assert_ne!(result.post_edit_sources[0].observation.tag, receipt.tag);
}

#[test]
fn hashline_rejects_unseen_and_changed_anchors_without_writing() {
    let root = tempfile::tempdir().expect("workspace");
    let path = root.path().join("source.txt");
    fs::write(&path, "one\ntwo\nthree\n").expect("source");
    let workspace = Workspace::new(&root.path().to_string_lossy()).expect("workspace");
    let partial = workspace
        .read_file("source.txt", &[LineRange { start: 1, end: 1 }])
        .expect("partial observation");
    let receipt = partial.observation.expect("receipt");
    let unseen_hash = format!("{:02X}", short_line_hash(b"two"));
    let unseen = HashlineSection {
        path: "source.txt".to_owned(),
        tag: receipt.tag,
        operations: vec![put_range(2, &unseen_hash, 2, &unseen_hash, &["changed"])],
    };
    assert!(matches!(
        workspace.apply_hashline(&[unseen], &context(&workspace)),
        Err(WorkspaceError::UnseenLine(_))
    ));

    let complete = workspace.read_file("source.txt", &[]).expect("observation");
    let receipt = complete.observation.expect("receipt");
    fs::write(&path, "one\nmodified\nthree\n").expect("external change");
    let stale = HashlineSection {
        path: "source.txt".to_owned(),
        tag: receipt.tag,
        operations: vec![put_range(
            2,
            &complete.lines[1].short_hash,
            2,
            &complete.lines[1].short_hash,
            &["changed"],
        )],
    };
    assert!(matches!(
        workspace.apply_hashline(&[stale], &context(&workspace)),
        Err(WorkspaceError::ObservedLineChanged(_))
    ));
    assert_eq!(
        fs::read_to_string(path).expect("source"),
        "one\nmodified\nthree\n"
    );
}

#[test]
fn hashline_resolves_cross_file_registers_in_authored_order() {
    let root = tempfile::tempdir().expect("workspace");
    fs::write(root.path().join("source.txt"), "move me").expect("source");
    fs::write(root.path().join("target.txt"), "target\n").expect("target");
    let workspace = Workspace::new(&root.path().to_string_lossy()).expect("workspace");
    let source = workspace
        .read_file("source.txt", &[])
        .expect("source observation");
    let target = workspace
        .read_file("target.txt", &[])
        .expect("target observation");

    workspace
        .apply_hashline(
            &[
                HashlineSection {
                    path: "source.txt".to_owned(),
                    tag: source.observation.expect("source receipt").tag,
                    operations: vec![cut_range(
                        1,
                        &source.lines[0].short_hash,
                        1,
                        &source.lines[0].short_hash,
                        "moved",
                    )],
                },
                HashlineSection {
                    path: "target.txt".to_owned(),
                    tag: target.observation.expect("target receipt").tag,
                    operations: vec![put_end(&target.lines[0].short_hash, "moved")],
                },
            ],
            &context(&workspace),
        )
        .expect("hashline");

    assert_eq!(
        fs::read_to_string(root.path().join("source.txt")).expect("source"),
        ""
    );
    assert_eq!(
        fs::read_to_string(root.path().join("target.txt")).expect("target"),
        "target\nmove me"
    );
}

#[test]
fn hashline_rejects_duplicate_normalized_paths_before_committing() {
    let root = tempfile::tempdir().expect("workspace");
    let path = root.path().join("source.txt");
    fs::write(&path, "one\n").expect("source");
    let workspace = Workspace::new(&root.path().to_string_lossy()).expect("workspace");
    let read = workspace.read_file("source.txt", &[]).expect("observation");
    let receipt = read.observation.expect("receipt");
    let operation = put_range(
        1,
        &read.lines[0].short_hash,
        1,
        &read.lines[0].short_hash,
        &["changed"],
    );

    let error = workspace
        .apply_hashline(
            &[
                HashlineSection {
                    path: "source.txt".to_owned(),
                    tag: receipt.tag.clone(),
                    operations: vec![operation.clone()],
                },
                HashlineSection {
                    path: "./source.txt".to_owned(),
                    tag: receipt.tag,
                    operations: vec![operation],
                },
            ],
            &context(&workspace),
        )
        .expect_err("duplicate normalized paths must fail");

    assert!(matches!(error, WorkspaceError::Patch(message) if message.contains("duplicate")));
    assert_eq!(fs::read_to_string(path).expect("source"), "one\n");
}

#[test]
fn hashline_rejects_normalized_destination_conflicts_before_committing() {
    let root = tempfile::tempdir().expect("workspace");
    let source_path = root.path().join("source.txt");
    let target_path = root.path().join("target.txt");
    fs::write(&source_path, "source\n").expect("source");
    fs::write(&target_path, "target\n").expect("target");
    let workspace = Workspace::new(&root.path().to_string_lossy()).expect("workspace");
    let source = workspace
        .read_file("source.txt", &[])
        .expect("source observation");
    let target = workspace
        .read_file("target.txt", &[])
        .expect("target observation");
    let move_operation = HashlineOperation {
        kind: "move".to_owned(),
        start: None,
        start_hash: None,
        end: None,
        end_hash: None,
        body: Vec::new(),
        register: None,
        destination: Some("./target.txt".to_owned()),
    };

    let error = workspace
        .apply_hashline(
            &[
                HashlineSection {
                    path: "./source.txt".to_owned(),
                    tag: source.observation.expect("receipt").tag,
                    operations: vec![move_operation],
                },
                HashlineSection {
                    path: "target.txt".to_owned(),
                    tag: target.observation.expect("receipt").tag,
                    operations: vec![put_range(
                        1,
                        &target.lines[0].short_hash,
                        1,
                        &target.lines[0].short_hash,
                        &["changed"],
                    )],
                },
            ],
            &context(&workspace),
        )
        .expect_err("normalized destination conflict");

    assert!(matches!(error, WorkspaceError::Patch(message) if message.contains("conflicting")));
    assert_eq!(fs::read_to_string(source_path).expect("source"), "source\n");
    assert_eq!(fs::read_to_string(target_path).expect("target"), "target\n");
}

#[test]
fn destructive_hashline_precondition_rejects_same_content_replacement() {
    let root = tempfile::tempdir().expect("workspace");
    let path = root.path().join("source.txt");
    fs::write(&path, "one\n").expect("source");
    let expected = sha256(b"one\n");
    let mut identity = file_identity(&path, "source.txt").expect("identity");
    fs::remove_file(&path).expect("remove original");
    fs::write(&path, "one\n").expect("replacement");

    assert!(matches!(
        ensure_current_file(&path, "source.txt", &expected, &mut identity),
        Err(WorkspaceError::Stale(_))
    ));
}

#[test]
fn hashline_uses_normalized_observation_paths() {
    let root = tempfile::tempdir().expect("workspace");
    let path = root.path().join("source.txt");
    fs::write(&path, "one\n").expect("source");
    let workspace = Workspace::new(&root.path().to_string_lossy()).expect("workspace");
    let read = workspace
        .read_file("./source.txt", &[])
        .expect("observation");
    let receipt = read.observation.expect("receipt");
    let operation = put_range(
        1,
        &read.lines[0].short_hash,
        1,
        &read.lines[0].short_hash,
        &["changed"],
    );

    workspace
        .apply_hashline(
            &[HashlineSection {
                path: "./source.txt".to_owned(),
                tag: receipt.tag,
                operations: vec![operation],
            }],
            &context(&workspace),
        )
        .expect("hashline");

    assert_eq!(fs::read_to_string(path).expect("source"), "changed\n");
}

#[test]
fn hashline_requires_complete_evidence_for_remove_and_commits_move_after_edit() {
    let root = tempfile::tempdir().expect("workspace");
    let remove_path = root.path().join("remove.txt");
    let move_path = root.path().join("move.txt");
    fs::write(&remove_path, "one\ntwo\n").expect("remove source");
    fs::write(&move_path, "before\n").expect("move source");
    let workspace = Workspace::new(&root.path().to_string_lossy()).expect("workspace");
    let partial = workspace
        .read_file("remove.txt", &[LineRange { start: 1, end: 1 }])
        .expect("partial observation");
    let remove = HashlineOperation {
        kind: "remove".to_owned(),
        start: None,
        start_hash: None,
        end: None,
        end_hash: None,
        body: Vec::new(),
        register: None,
        destination: None,
    };
    let incomplete = HashlineSection {
        path: "remove.txt".to_owned(),
        tag: partial.observation.expect("receipt").tag,
        operations: vec![remove.clone()],
    };
    assert!(matches!(
        workspace.apply_hashline(&[incomplete], &context(&workspace)),
        Err(WorkspaceError::UnseenLine(_))
    ));
    assert!(remove_path.exists());

    let complete = workspace
        .read_file("remove.txt", &[])
        .expect("complete observation");
    let removed = workspace
        .apply_hashline(
            &[HashlineSection {
                path: "remove.txt".to_owned(),
                tag: complete.observation.expect("receipt").tag,
                operations: vec![remove],
            }],
            &context(&workspace),
        )
        .expect("remove");
    assert!(!remove_path.exists());
    assert_eq!(removed.changes[0].operation, "delete");

    let observed = workspace
        .read_file("move.txt", &[])
        .expect("move observation");
    let move_operation = HashlineOperation {
        kind: "move".to_owned(),
        start: None,
        start_hash: None,
        end: None,
        end_hash: None,
        body: Vec::new(),
        register: None,
        destination: Some("./moved.txt".to_owned()),
    };
    let moved = workspace
        .apply_hashline(
            &[HashlineSection {
                path: "./move.txt".to_owned(),
                tag: observed.observation.expect("receipt").tag,
                operations: vec![
                    put_range(
                        1,
                        &observed.lines[0].short_hash,
                        1,
                        &observed.lines[0].short_hash,
                        &["after"],
                    ),
                    move_operation,
                ],
            }],
            &context(&workspace),
        )
        .expect("edit and move");
    assert!(!move_path.exists());
    assert_eq!(
        fs::read_to_string(root.path().join("moved.txt")).expect("destination"),
        "after\n"
    );
    assert_eq!(moved.changes[0].operation, "move");
    assert_eq!(moved.changes[0].path, "move.txt");
    assert_eq!(moved.changes[0].destination.as_deref(), Some("moved.txt"));
    assert_eq!(moved.post_edit_sources[0].path, "moved.txt");
}
