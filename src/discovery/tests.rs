use std::fs;
use std::time::Duration;

use crate::discovery::{
    DiscoveryError, NamedFileRequest, discover_named_files, find_ancestor_entry, read_text_files,
};
use crate::workspace::{Cancellation, WorkCompletion, WorkControl};

fn request(limit: usize) -> NamedFileRequest {
    NamedFileRequest {
        filename: "AGENTS.md".to_owned(),
        max_depth: 4,
        limit,
    }
}

#[test]
fn finds_ancestor_entry_for_directory_and_file_markers() {
    let root = tempfile::tempdir().expect("root");
    let nested = root.path().join("one/two");
    fs::create_dir_all(&nested).expect("nested directory");
    fs::create_dir(root.path().join(".git")).expect("directory marker");

    let found = find_ancestor_entry(&nested, ".git").expect("ancestor lookup");
    assert_eq!(found.as_deref(), Some(root.path()));

    fs::remove_dir(root.path().join(".git")).expect("remove directory marker");
    fs::write(root.path().join(".git"), "gitdir: elsewhere").expect("file marker");
    let found = find_ancestor_entry(&nested, ".git").expect("worktree ancestor lookup");
    assert_eq!(found.as_deref(), Some(root.path()));
    assert!(matches!(
        find_ancestor_entry(&nested, "../.git"),
        Err(DiscoveryError::Configuration(_))
    ));
}

#[test]
fn reads_optional_utf8_files_in_input_order() {
    let root = tempfile::tempdir().expect("root");
    let first = root.path().join("first.txt");
    let missing = root.path().join("missing.txt");
    let second = root.path().join("second.txt");
    fs::write(&first, "first\r\nline\rending").expect("first file");
    fs::write(&second, "second").expect("second file");
    let paths = vec![
        first.to_string_lossy().into_owned(),
        missing.to_string_lossy().into_owned(),
        second.to_string_lossy().into_owned(),
    ];

    let files = read_text_files(paths.clone(), &WorkControl::new(Cancellation::new(), None))
        .expect("text files");
    assert_eq!(
        files,
        [
            (paths[0].clone(), "first\nline\nending".to_owned()),
            (paths[2].clone(), "second".to_owned())
        ]
    );

    fs::write(&second, [0xff]).expect("invalid UTF-8 file");
    assert!(matches!(
        read_text_files(paths, &WorkControl::new(Cancellation::new(), None)),
        Err(DiscoveryError::Encoding(_))
    ));
}

#[test]
fn discovers_named_files_with_ignore_and_generated_directory_policy() {
    let root = tempfile::tempdir().expect("root");
    fs::create_dir(root.path().join(".git")).expect("repository marker");
    fs::write(root.path().join(".gitignore"), "AGENTS.md\nvendor/\n").expect("ignore rules");
    for directory in [
        "src",
        "src/nested",
        "vendor",
        "coverage",
        ".hidden",
        "one/two/three/four/five",
        "node_modules/package",
    ] {
        fs::create_dir_all(root.path().join(directory)).expect("context directory");
        fs::write(root.path().join(directory).join("AGENTS.md"), directory).expect("context file");
    }
    fs::write(root.path().join("AGENTS.md"), "root").expect("root context");

    let result = discover_named_files(
        root.path(),
        &request(200),
        &WorkControl::new(Cancellation::new(), None),
    )
    .expect("discovery");

    assert_eq!(result.paths, ["src/AGENTS.md", "src/nested/AGENTS.md"]);
    assert_eq!(result.completion, WorkCompletion::Complete);
}

#[test]
fn named_file_discovery_reports_limits_deadlines_and_cancellation() {
    let root = tempfile::tempdir().expect("root");
    for directory in ["first", "second"] {
        fs::create_dir(root.path().join(directory)).expect("context directory");
        fs::write(root.path().join(directory).join("AGENTS.md"), directory).expect("context file");
    }

    let limited = discover_named_files(
        root.path(),
        &request(1),
        &WorkControl::new(Cancellation::new(), None),
    )
    .expect("limited discovery");
    assert_eq!(limited.paths, ["first/AGENTS.md"]);
    assert_eq!(limited.completion, WorkCompletion::FileLimitReached);

    let deadline = discover_named_files(
        root.path(),
        &request(200),
        &WorkControl::new(Cancellation::new(), Some(Duration::ZERO)),
    )
    .expect("deadline result");
    assert!(deadline.paths.is_empty());
    assert_eq!(deadline.completion, WorkCompletion::DeadlineReached);

    let cancellation = Cancellation::new();
    cancellation.cancel();
    let cancelled = discover_named_files(
        root.path(),
        &request(200),
        &WorkControl::new(cancellation, None),
    );
    assert!(matches!(cancelled, Err(DiscoveryError::Cancelled)));
}
