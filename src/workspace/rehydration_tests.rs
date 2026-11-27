use std::fs;

use crate::workspace::{RehydrationStatus, Workspace, WorkspaceError, WorkspacePolicy, sha256};

fn workspace_with(content: &str) -> (tempfile::TempDir, Workspace, String) {
    let root = tempfile::tempdir().expect("workspace");
    fs::write(root.path().join("source.txt"), content).expect("source");
    let workspace = Workspace::new(&root.path().to_string_lossy()).expect("workspace");
    let digest = sha256(content.as_bytes());
    (root, workspace, digest)
}

#[test]
fn matching_digest_records_exactly_the_given_lines() {
    let (_root, workspace, digest) = workspace_with("one\ntwo\nthree\n");

    let status = workspace
        .rehydrate_observation("source.txt", &digest, &[3, 1, 1], true)
        .expect("rehydrate");

    assert_eq!(status, RehydrationStatus::Restored);
    let receipt = workspace
        .validate_observed_lines("source.txt", &digest[..4], &[1, 3])
        .expect("restored lines");
    assert!(!receipt.complete_presentation);
    let unseen = workspace.validate_observed_lines("source.txt", &digest[..4], &[2]);
    assert!(matches!(unseen, Err(WorkspaceError::UnseenLine(_))));
}

#[test]
fn complete_presentation_requires_every_line() {
    let (_root, workspace, digest) = workspace_with("one\ntwo\n");

    workspace
        .rehydrate_observation("source.txt", &digest, &[1, 2], true)
        .expect("rehydrate");

    let receipt = workspace
        .resolve_observation("source.txt", &digest[..4])
        .expect("restored receipt");
    assert!(receipt.complete_presentation);
}

#[test]
fn stale_receipts_record_nothing() {
    let (_root, workspace, digest) = workspace_with("one\ntwo\n");
    let other = sha256(b"changed\n");

    for (path, sha, lines) in [
        ("source.txt", other.as_str(), &[1][..]),
        ("missing.txt", digest.as_str(), &[1][..]),
        ("../outside.txt", digest.as_str(), &[1][..]),
        ("source.txt", digest.as_str(), &[1, 3][..]),
        ("source.txt", digest.as_str(), &[0][..]),
    ] {
        let status = workspace
            .rehydrate_observation(path, sha, lines, false)
            .expect("rehydrate");
        assert_eq!(status, RehydrationStatus::Stale, "{path} {lines:?}");
    }

    let missing = workspace.resolve_observation("source.txt", &digest[..4]);
    assert!(matches!(
        missing,
        Err(WorkspaceError::ObservationNotFound(_))
    ));
}

#[test]
fn full_ledger_skips_without_evicting() {
    let (root, workspace, first) = workspace_with("one\n");
    fs::write(root.path().join("other.txt"), "two\n").expect("other");
    let second = sha256(b"two\n");
    workspace
        .set_policy(WorkspacePolicy {
            max_observation_entries: 1,
            ..WorkspacePolicy::default()
        })
        .expect("policy");

    let restored = workspace
        .rehydrate_observation("source.txt", &first, &[1], false)
        .expect("first");
    let merged = workspace
        .rehydrate_observation("source.txt", &first, &[1], false)
        .expect("same entry");
    let skipped = workspace
        .rehydrate_observation("other.txt", &second, &[1], false)
        .expect("second");

    assert_eq!(restored, RehydrationStatus::Restored);
    assert_eq!(merged, RehydrationStatus::Restored);
    assert_eq!(skipped, RehydrationStatus::Skipped);
    workspace
        .validate_observed_lines("source.txt", &first[..4], &[1])
        .expect("first entry kept");
}

#[test]
fn closed_workspace_raises() {
    let (_root, workspace, digest) = workspace_with("one\n");
    workspace.close();

    let closed = workspace.rehydrate_observation("source.txt", &digest, &[1], false);

    assert!(matches!(closed, Err(WorkspaceError::Closed)));
}
