use std::fs;

use crate::workspace::parse_apply_patch;

use super::{
    CrashPoint, context, crash_at, entries, fixture, hashline, open, paths, read, recover, section,
};

#[test]
fn hashline_crash_after_the_first_of_three_files_rolls_back_on_open() {
    let files = [("a.txt", "a\n"), ("b.txt", "b\n"), ("c.txt", "c\n")];
    let (root, state) = fixture(&files);
    let workspace = open(&root, &state);
    let sections =
        ["a.txt", "b.txt", "c.txt"].map(|path| section(&workspace, path, "put_range", "changed"));

    crash_at(CrashPoint::Landed(0), || hashline(&workspace, &sections));

    assert_eq!(read(&root, "a.txt"), "changed\n");
    assert_eq!(read(&root, "b.txt"), "b\n");
    assert_eq!(entries(&state.path().join("mutations")).len(), 1);
    assert_eq!(
        recover(&root, &state),
        vec![(paths(&["a.txt"]), Vec::new())]
    );
    for (path, content) in files {
        assert_eq!(read(&root, path), content);
    }
}

#[test]
fn patch_crash_rolls_back_updates_creates_and_deletes_on_open() {
    for (crash, restored) in [
        (0, paths(&["a.txt"])),
        (2, paths(&["a.txt", "new.txt", "c.txt"])),
    ] {
        let (root, state) = fixture(&[("a.txt", "one\n"), ("c.txt", "gone\n")]);
        let workspace = open(&root, &state);
        workspace.read_file("a.txt", &[]).expect("observation");
        workspace.read_file("c.txt", &[]).expect("observation");
        let operations = parse_apply_patch(
            "*** Begin Patch\n*** Update File: a.txt\n@@\n-one\n+changed\n*** Add File: new.txt\n+new\n*** Delete File: c.txt\n*** End Patch",
        )
        .expect("patch");

        crash_at(CrashPoint::Landed(crash), || {
            workspace.apply_patch_operations(
                &operations,
                &context(&workspace, "apply_patch"),
                "apply_patch",
            )
        });

        assert_eq!(read(&root, "a.txt"), "changed\n");
        assert_eq!(recover(&root, &state), vec![(restored, Vec::new())]);
        assert_eq!(read(&root, "a.txt"), "one\n");
        assert_eq!(read(&root, "c.txt"), "gone\n");
        assert!(!root.path().join("new.txt").exists());
    }
}

#[test]
fn staged_hashline_remove_crash_restores_the_original_on_open() {
    let (root, state) = fixture(&[("remove.txt", "keep\n")]);
    let workspace = open(&root, &state);
    let sections = [section(&workspace, "remove.txt", "remove", "")];

    crash_at(CrashPoint::Staged, || hashline(&workspace, &sections));

    assert!(!root.path().join("remove.txt").exists());
    assert_eq!(
        recover(&root, &state),
        vec![(paths(&["remove.txt"]), Vec::new())]
    );
    assert_eq!(read(&root, "remove.txt"), "keep\n");
}

#[test]
fn landed_hashline_move_rolls_back_on_open() {
    let (root, state) = fixture(&[("source.txt", "move\n")]);
    let workspace = open(&root, &state);
    let sections = [section(&workspace, "source.txt", "move", "target.txt")];

    crash_at(CrashPoint::Landed(0), || hashline(&workspace, &sections));

    assert_eq!(read(&root, "target.txt"), "move\n");
    assert_eq!(
        recover(&root, &state),
        vec![(paths(&["source.txt"]), Vec::new())]
    );
    assert_eq!(read(&root, "source.txt"), "move\n");
    assert!(!root.path().join("target.txt").exists());
}

#[test]
fn external_change_after_a_crash_is_reported_once_and_kept() {
    let (root, state) = fixture(&[("a.txt", "a\n"), ("b.txt", "b\n")]);
    let workspace = open(&root, &state);
    let sections = ["a.txt", "b.txt"].map(|path| section(&workspace, path, "put_range", "changed"));
    crash_at(CrashPoint::Landed(0), || hashline(&workspace, &sections));
    fs::write(root.path().join("a.txt"), "external\n").expect("external edit");

    assert_eq!(
        recover(&root, &state),
        vec![(Vec::new(), paths(&["a.txt"]))]
    );
    assert_eq!(read(&root, "a.txt"), "external\n");
    assert_eq!(read(&root, "b.txt"), "b\n");
    assert!(recover(&root, &state).is_empty());
}
