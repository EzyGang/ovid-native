use std::cell::Cell;
use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use tempfile::TempDir;

use crate::workspace::hashline_types::{HashlineOperation, HashlineSection};
use crate::workspace::{Cancellation, MutationContext, Workspace};

mod rollback;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CrashPoint {
    Landed(usize),
    Staged,
}

thread_local! {
    static CRASH_POINT: Cell<Option<CrashPoint>> = const { Cell::new(None) };
}

pub(crate) fn crash_requested(point: CrashPoint) -> bool {
    CRASH_POINT.with(|armed| armed.get() == Some(point))
}

fn crash_at<F, T>(point: CrashPoint, operation: F)
where
    F: FnOnce() -> T,
{
    CRASH_POINT.with(|armed| armed.set(Some(point)));
    let outcome = catch_unwind(AssertUnwindSafe(operation));
    CRASH_POINT.with(|armed| armed.set(None));
    assert!(outcome.is_err(), "the mutation should stop at {point:?}");
}

fn fixture(files: &[(&str, &str)]) -> (TempDir, TempDir) {
    let root = tempfile::tempdir().expect("workspace");
    let state = tempfile::tempdir().expect("state");
    for (path, content) in files {
        fs::write(root.path().join(path), content).expect("source");
    }
    (root, state)
}

fn open(root: &TempDir, state: &TempDir) -> Workspace {
    Workspace::new(
        &root.path().to_string_lossy(),
        Some(state.path().to_path_buf()),
    )
    .expect("workspace")
}

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

fn section(workspace: &Workspace, path: &str, kind: &str, argument: &str) -> HashlineSection {
    let read = workspace.read_file(path, &[]).expect("observation");
    let hash = read.lines[0].short_hash.clone();
    let ranged = kind == "put_range";
    HashlineSection {
        path: path.to_owned(),
        tag: read.observation.expect("receipt").tag,
        operations: vec![HashlineOperation {
            kind: kind.to_owned(),
            start: ranged.then_some(1),
            start_hash: ranged.then(|| hash.clone()),
            end: ranged.then_some(1),
            end_hash: ranged.then_some(hash),
            body: if ranged {
                vec![argument.to_owned()]
            } else {
                Vec::new()
            },
            register: None,
            destination: (kind == "move").then(|| argument.to_owned()),
        }],
    }
}

fn hashline(workspace: &Workspace, sections: &[HashlineSection]) {
    workspace
        .apply_hashline(sections, &context(workspace, "hashline"))
        .expect("hashline");
}

fn read(root: &TempDir, path: &str) -> String {
    fs::read_to_string(root.path().join(path)).expect("file")
}

fn entries(directory: &Path) -> Vec<String> {
    fs::read_dir(directory).map_or_else(
        |_| Vec::new(),
        |listing| {
            listing
                .map(|item| {
                    item.expect("entry")
                        .file_name()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect()
        },
    )
}

fn recover(root: &TempDir, state: &TempDir) -> Vec<(Vec<String>, Vec<String>)> {
    let reports = open(root, state).recover().expect("recovery");
    assert!(entries(&state.path().join("mutations")).is_empty());
    assert!(entries(&state.path().join("blobs")).is_empty());
    assert!(
        !entries(root.path())
            .iter()
            .any(|name| name.starts_with(".ovid-hashline-"))
    );
    reports
        .into_iter()
        .map(|report| (report.restored, report.conflicts))
        .collect()
}

fn paths(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn finished_mutation_keeps_blobs_that_a_leftover_record_names() {
    let (root, state) = fixture(&[("a.txt", "same\n"), ("b.txt", "same\n")]);
    let crashed = open(&root, &state);
    let sections = [section(&crashed, "a.txt", "put_range", "changed")];
    crash_at(CrashPoint::Landed(0), || hashline(&crashed, &sections));

    let workspace = open(&root, &state);
    hashline(
        &workspace,
        &[section(&workspace, "b.txt", "put_range", "edited")],
    );

    assert_eq!(read(&root, "b.txt"), "edited\n");
    assert_eq!(entries(&state.path().join("mutations")).len(), 1);
    assert_eq!(entries(&state.path().join("blobs")).len(), 1);
    assert_eq!(
        recover(&root, &state),
        vec![(paths(&["a.txt"]), Vec::new())]
    );
    assert_eq!(read(&root, "a.txt"), "same\n");
}

#[test]
fn workspace_without_a_state_dir_does_not_journal() {
    let (root, state) = fixture(&[("a.txt", "a\n")]);
    let workspace = Workspace::new(&root.path().to_string_lossy(), None).expect("workspace");
    hashline(
        &workspace,
        &[section(&workspace, "a.txt", "put_range", "changed")],
    );

    assert_eq!(read(&root, "a.txt"), "changed\n");
    assert!(workspace.recover().expect("recovery").is_empty());
    assert!(entries(state.path()).is_empty());
}
