use crate::workspace::directory_tree::build_directory_tree;
use crate::workspace::{
    Workspace, WorkspaceDirectoryEntry, WorkspaceDirectoryRead, WorkspaceError,
};

#[test]
fn tree_is_hierarchical_deterministic_and_limits_nested_children() {
    let mut entries = vec![WorkspaceDirectoryEntry {
        path: "src/pkg".to_owned(),
        kind: "directory".to_owned(),
        size: None,
    }];
    for index in (0..14).rev() {
        entries.push(WorkspaceDirectoryEntry {
            path: format!("src/pkg/child-{index:02}.py"),
            kind: "file".to_owned(),
            size: Some(1),
        });
    }
    for index in 0..14 {
        entries.push(WorkspaceDirectoryEntry {
            path: format!("src/root-{index:02}.py"),
            kind: "file".to_owned(),
            size: Some(1),
        });
    }

    let tree = build_directory_tree(
        WorkspaceDirectoryRead {
            path: "src".to_owned(),
            entries,
            truncated: false,
        },
        12,
    );

    assert_eq!(tree.scanned_entries, 29);
    assert_eq!(tree.limited_directories, 1);
    assert_eq!(tree.omitted_entries, 2);
    assert_eq!(tree.lines[0].name, "pkg");
    assert_eq!(tree.lines[0].depth, 0);
    assert_eq!(tree.lines[12].kind, "omitted");
    assert_eq!(tree.lines[12].omitted_entries, Some(2));
    assert_eq!(tree.lines[13].name, "child-13.py");
    assert_eq!(tree.lines[14].name, "root-00.py");
    assert_eq!(
        tree.lines.last().map(|line| line.name.as_str()),
        Some("root-13.py")
    );
    assert!(!tree.scan_truncated);
}

#[test]
fn workspace_rejects_invalid_directory_child_limits() {
    let root = tempfile::tempdir().expect("workspace");
    let workspace = Workspace::new(&root.path().to_string_lossy()).expect("workspace");

    assert!(matches!(
        workspace.read_directory_tree(".", 1, 0),
        Err(WorkspaceError::Limit(_))
    ));
}
