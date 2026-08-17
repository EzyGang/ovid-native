use std::fs;

use crate::workspace::hashline_types::{HashlineOperation, HashlineSection};
use crate::workspace::line_hash::short_line_hash;
use crate::workspace::{
    Cancellation, LineRange, MutationContext, Workspace, WorkspaceError, ensure_current_path,
    sha256,
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
    fs::write(root.path().join("source.txt"), "move me\n").expect("source");
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
        "target\nmove me\n"
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
fn destructive_hashline_precondition_rejects_changed_source() {
    let root = tempfile::tempdir().expect("workspace");
    let path = root.path().join("source.txt");
    fs::write(&path, "one\n").expect("source");
    let expected = sha256(b"one\n");
    fs::write(&path, "changed\n").expect("external change");

    assert!(matches!(
        ensure_current_path(&path, "source.txt", &expected),
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
