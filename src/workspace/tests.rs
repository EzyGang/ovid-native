use std::fs;
use std::time::Duration;

use crate::workspace::{
    Cancellation, MetadataLevel, ReadExtent, ScanFileKind, ScanOrder, ScanRequest, WorkCompletion,
    WorkControl, Workspace, WorkspaceError, preflight_write, read_content, replace_file, sha256,
};

fn scan_request(selections: &[&str]) -> ScanRequest {
    ScanRequest {
        selections: selections.iter().map(|value| (*value).to_owned()).collect(),
        include_hidden: false,
        respect_gitignore: true,
        include_node_modules: false,
        file_kind: ScanFileKind::Files,
        metadata: MetadataLevel::Size,
        order: ScanOrder::Path,
        max_files: 100,
    }
}

#[test]
fn workspace_scans_exact_directory_glob_and_multiple_selections() {
    let root = tempfile::tempdir().expect("workspace");
    fs::create_dir_all(root.path().join("src/nested")).expect("source directories");
    fs::write(root.path().join("src/a.rs"), "a").expect("a");
    fs::write(root.path().join("src/nested/b.rs"), "bb").expect("b");
    let workspace = Workspace::new(&root.path().to_string_lossy()).expect("workspace");
    let control = WorkControl::new(Cancellation::new(), None);

    let result = workspace
        .scan(&scan_request(&["src", "src/**/*.rs"]), &control)
        .expect("scan");
    let paths = result
        .entries
        .iter()
        .map(|entry| entry.relative.as_str())
        .collect::<Vec<_>>();

    assert_eq!(paths, ["src/a.rs", "src/nested/b.rs"]);
    assert_eq!(result.entries[0].size, Some(1));
    assert_eq!(result.completion, WorkCompletion::Complete);
}

#[test]
fn workspace_rejects_traversal_and_reports_limits() {
    let root = tempfile::tempdir().expect("workspace");
    fs::write(root.path().join("a.txt"), "a").expect("a");
    fs::write(root.path().join("b.txt"), "b").expect("b");
    let workspace = Workspace::new(&root.path().to_string_lossy()).expect("workspace");
    let control = WorkControl::new(Cancellation::new(), None);

    let traversal = workspace.scan(&scan_request(&["../outside"]), &control);
    assert!(matches!(traversal, Err(WorkspaceError::Path(_))));

    let mut limited = scan_request(&["."]);
    limited.max_files = 1;
    let result = workspace.scan(&limited, &control).expect("limited scan");
    assert_eq!(result.entries.len(), 1);
    assert_eq!(result.completion, WorkCompletion::FileLimitReached);
}

#[test]
fn workspace_reports_deadline_and_cancellation() {
    let root = tempfile::tempdir().expect("workspace");
    let workspace = Workspace::new(&root.path().to_string_lossy()).expect("workspace");
    let deadline = WorkControl::new(Cancellation::new(), Some(Duration::ZERO));
    let result = workspace
        .scan(&scan_request(&["."]), &deadline)
        .expect("deadline result");
    assert_eq!(result.completion, WorkCompletion::DeadlineReached);

    let cancellation = Cancellation::new();
    cancellation.cancel();
    let cancelled = workspace.scan(&scan_request(&["."]), &WorkControl::new(cancellation, None));
    assert!(matches!(cancelled, Err(WorkspaceError::Cancelled)));
}

#[test]
fn workspace_classifies_bounded_content() {
    let root = tempfile::tempdir().expect("workspace");
    let path = root.path().join("content.bin");
    fs::write(&path, b"abc\0def").expect("content");
    let control = WorkControl::new(Cancellation::new(), None);

    let prefix =
        read_content(&path, ReadExtent::Prefix { max_bytes: 4 }, &control).expect("prefix");
    assert_eq!(prefix.searched_bytes, 4);
    assert_eq!(prefix.total_bytes, 7);
    assert!(!prefix.complete);
    assert!(prefix.binary);

    let complete =
        read_content(&path, ReadExtent::Complete { max_bytes: 10 }, &control).expect("complete");
    assert!(complete.complete);
}

#[test]
fn workspace_preflights_and_atomically_replaces_content() {
    let root = tempfile::tempdir().expect("workspace");
    let path = root.path().join("source.txt");
    fs::write(&path, "before").expect("source");
    let workspace = Workspace::new(&root.path().to_string_lossy()).expect("workspace");
    let prepared =
        preflight_write(workspace.root(), "source.txt", &sha256(b"before")).expect("preflight");

    replace_file(&prepared, b"after").expect("replace");

    assert_eq!(fs::read_to_string(path).expect("updated source"), "after");
}

#[test]
fn workspace_unordered_full_metadata_includes_directories() {
    let root = tempfile::tempdir().expect("workspace");
    fs::create_dir(root.path().join("src")).expect("directory");
    let workspace = Workspace::new(&root.path().to_string_lossy()).expect("workspace");
    let mut request = scan_request(&["src"]);
    request.file_kind = ScanFileKind::FilesAndDirectories;
    request.metadata = MetadataLevel::Full;
    request.order = ScanOrder::Unordered;

    let result = workspace
        .scan(&request, &WorkControl::new(Cancellation::new(), None))
        .expect("scan");

    assert_eq!(result.entries[0].relative, "src");
    assert!(result.entries[0].modified.is_some());
}

#[cfg(unix)]
#[test]
fn workspace_prunes_directory_symlinks_and_rejects_external_file_symlinks() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().expect("workspace");
    let outside = tempfile::tempdir().expect("outside");
    fs::write(outside.path().join("outside.txt"), "outside").expect("outside file");
    symlink(outside.path(), root.path().join("linked-dir")).expect("directory link");
    symlink(
        outside.path().join("outside.txt"),
        root.path().join("linked-file.txt"),
    )
    .expect("file link");
    let workspace = Workspace::new(&root.path().to_string_lossy()).expect("workspace");
    let control = WorkControl::new(Cancellation::new(), None);

    assert!(matches!(
        workspace.scan(&scan_request(&["linked-dir"]), &control),
        Err(WorkspaceError::Path(_))
    ));
    assert!(matches!(
        workspace.scan(&scan_request(&["linked-file.txt"]), &control),
        Err(WorkspaceError::Path(_))
    ));
}
