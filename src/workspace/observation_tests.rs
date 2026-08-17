use std::fs;

use crate::workspace::{Workspace, WorkspaceError, line_hash::short_line_hash};

#[test]
fn short_line_hash_matches_xxh32_reference_low_bytes() {
    assert_eq!(short_line_hash(b""), 0x05);
    assert_eq!(short_line_hash(b"a"), 0x56);
    assert_eq!(short_line_hash(b"hello"), 0xf9);
}

#[test]
fn source_evidence_validates_utf8_spans_and_complete_line_claims() {
    let root = tempfile::tempdir().expect("workspace");
    fs::write(root.path().join("source.txt"), "one\nβeta\n").expect("source");
    let workspace = Workspace::new(&root.path().to_string_lossy()).expect("workspace");
    let claims = [(1, "one".to_owned()), (2, "βeta".to_owned())];

    workspace
        .observe_source_lines("source.txt", &claims, &[(1, 0, 2, 9)], true)
        .expect("valid source span");
    let invalid_utf8 = workspace.observe_source_lines("source.txt", &claims, &[(2, 4, 2, 5)], true);
    assert!(matches!(
        invalid_utf8,
        Err(WorkspaceError::ObservationNotFound(_))
    ));
    let missing_claim =
        workspace.observe_source_lines("source.txt", &claims[..1], &[(1, 0, 2, 9)], false);
    assert!(matches!(
        missing_claim,
        Err(WorkspaceError::ObservationNotFound(_))
    ));
}
