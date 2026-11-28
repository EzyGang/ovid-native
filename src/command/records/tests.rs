use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ovid_command_process::{ProcessIdentity, ProcessScope, boot_id, stop_orphans};
use tempfile::TempDir;

use super::reap::ReapedProcess;
use super::{
    HOST, HostRecord, ScopeEntry, ScopeKind, reap_orphans, terminate, track, write_record,
};
use crate::command::COMMAND_RUNTIME;
use crate::workspace::{Workspace, sha256};

#[test]
fn host_record_round_trips_through_its_encoding() {
    let mut record = record(
        identity(41, 7),
        vec![entry(
            Some("call\t1\nä"),
            vec![identity(77, 1), identity(78, 2)],
        )],
    );
    record.entries.push(ScopeEntry {
        kind: ScopeKind::Job,
        owner_tag: None,
        group: None,
        ..entry(None, vec![identity(5, 3)])
    });

    assert_eq!(HostRecord::decode(&record.encode()), Some(record.clone()));
    record.boot_id = None;
    assert_eq!(HostRecord::decode(&record.encode()), Some(record));
}

#[test]
fn malformed_records_are_rejected() {
    let valid = record(
        identity(41, 7),
        vec![entry(Some("tag"), vec![identity(77, 1)])],
    )
    .encode();
    let cases = [
        String::new(),
        valid.replace("ovid-process-record 1", "ovid-process-record 2"),
        valid.replace("\tcommand\t", "\tdaemon\t"),
        valid.replace("77:1", "77:x"),
        valid.replace("\t746167\t", "\t74616\t"),
        format!("{valid}scope\tjob\n"),
    ];

    for case in cases {
        assert_eq!(HostRecord::decode(&case), None, "{case:?}");
    }
}

#[test]
fn write_record_replaces_the_file_and_deletes_it_without_entries() {
    let state = TempDir::new().expect("state directory should exist");
    let processes = state.path().join("processes");
    let mut record = record(identity(41, 7), vec![entry(None, vec![identity(77, 1)])]);
    let path = processes.join(HostRecord::file_name(record.host));

    write_record(&processes, &record).expect("record should be written");
    assert_eq!(read(&path), Some(record.clone()));
    record.entries = vec![entry(Some("second"), vec![identity(80, 4)])];
    write_record(&processes, &record).expect("record should be replaced");
    assert_eq!(read(&path), Some(record.clone()));

    record.entries.clear();
    write_record(&processes, &record).expect("empty record should be deleted");
    assert!(!path.exists());
    write_record(&processes, &record).expect("an absent record needs no deletion");
}

#[test]
fn identity_matches_only_the_same_live_process() {
    let mut child = sleeper().spawn().expect("child should start");
    let identity = ProcessIdentity::of(child.id()).expect("child should be alive");
    let reused = ProcessIdentity {
        start_time: identity.start_time + 1,
        ..identity
    };

    assert!(stop_orphans(None, &[reused], Duration::ZERO).is_empty());
    assert!(matches!(child.try_wait(), Ok(None)));
    assert_eq!(
        stop_orphans(None, &[identity], Duration::ZERO),
        vec![identity]
    );
    child.wait().expect("child should stop");
    assert_ne!(ProcessIdentity::of(child.id()), Some(identity));
}

#[cfg(unix)]
#[test]
fn stop_orphans_stops_every_process_of_a_proven_group() {
    let runtime = COMMAND_RUNTIME.as_ref().expect("runtime should start");
    let scope = ProcessScope::default();

    runtime.block_on(async {
        let (mut first, group) = scope.spawn(sleeper(), false).expect("child should start");
        let (mut second, _) = scope.spawn(sleeper(), false).expect("child should start");
        let recorded = ProcessIdentity::of(first.id().expect("child should have an ID"))
            .expect("child should be alive");

        assert_eq!(
            stop_orphans(group, &[recorded], Duration::from_millis(500)),
            vec![recorded]
        );
        tokio::time::timeout(Duration::from_secs(5), async {
            first.wait().await.expect("first child should stop");
            second.wait().await.expect("unrecorded child should stop");
        })
        .await
        .expect("the group should stop");
    });
}

#[test]
fn sampler_records_scope_members_until_the_scope_terminates() {
    let runtime = COMMAND_RUNTIME.as_ref().expect("runtime should start");
    let state = TempDir::new().expect("state directory should exist");
    let scope = Arc::new(ProcessScope::default());
    let host = HOST.as_ref().expect("host should have an identity").host;
    let processes = state.path().join("processes");
    let path = processes.join(HostRecord::file_name(host));

    track(
        Some(state.path()),
        &scope,
        ScopeKind::Job,
        Some("job"),
        "sleep",
    );
    let (_child, _) = runtime
        .block_on(async { scope.spawn(sleeper(), false) })
        .expect("child should start");
    let recorded = wait_for_record(&path);

    assert_eq!((recorded.host, recorded.boot_id.clone()), (host, boot_id()));
    let [entry] = recorded.entries.as_slice() else {
        panic!("record should hold one scope: {recorded:?}");
    };
    let tag = entry.owner_tag.as_deref();
    assert_eq!(
        (entry.kind, tag, entry.members.len()),
        (ScopeKind::Job, Some("job"), 1)
    );
    assert_eq!(entry.command_hash, sha256(b"sleep"));
    terminate(&scope, Duration::ZERO).expect("scope should terminate");
    assert!(!path.exists());
}

#[test]
fn reap_skips_live_hosts_and_stops_matching_members_of_dead_hosts() {
    let temporary = TempDir::new().expect("workspace should exist");
    let root = temporary.path().to_str().expect("path should be text");
    let state = temporary.path().join("state");
    let workspace = Workspace::new(root, Some(state.clone())).expect("workspace should open");
    let mut host = sleeper().spawn().expect("host should start");
    let mut orphan = sleeper().spawn().expect("orphan should start");
    let orphan_identity = ProcessIdentity::of(orphan.id()).expect("orphan should be alive");
    let host_identity = ProcessIdentity::of(host.id()).expect("host should be alive");
    let mut live = record(
        host_identity,
        vec![entry(Some("call"), vec![orphan_identity])],
    );
    live.boot_id = boot_id();
    write_record(&state.join("processes"), &live).expect("record should be written");

    let skipped = reap_orphans(&workspace).expect("reap should succeed");
    host.kill().expect("host should stop");
    host.wait().expect("host should exit");
    let reaped = reap_orphans(&workspace).expect("reap should succeed");

    assert_eq!((skipped.stopped.len(), skipped.skipped_live_owners), (0, 1));
    let expected = ReapedProcess {
        pid: orphan_identity.pid,
        owner_tag: Some("call".to_owned()),
        command_hash_prefix: sha256(b"sleep")[..12].to_owned(),
    };
    assert_eq!(reaped.stopped, vec![expected]);
    assert_eq!(
        fs::read_dir(state.join("processes"))
            .map(Iterator::count)
            .ok(),
        Some(0)
    );
    orphan.wait().expect("orphan should stop");
}

fn wait_for_record(path: &Path) -> HostRecord {
    let started = Instant::now();
    loop {
        if let Some(record) = read(path).filter(|record| !record.entries.is_empty()) {
            return record;
        }
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "record should appear"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn identity(pid: u32, start_time: u64) -> ProcessIdentity {
    ProcessIdentity { pid, start_time }
}

fn entry(owner_tag: Option<&str>, members: Vec<ProcessIdentity>) -> ScopeEntry {
    ScopeEntry {
        kind: ScopeKind::Command,
        owner_tag: owner_tag.map(str::to_owned),
        command_hash: sha256(b"sleep"),
        started_ms: 1_700_000_000_000,
        group: members
            .first()
            .and_then(|member| i32::try_from(member.pid).ok()),
        members,
    }
}

fn record(host: ProcessIdentity, entries: Vec<ScopeEntry>) -> HostRecord {
    HostRecord {
        host,
        boot_id: Some("boot".to_owned()),
        entries,
    }
}

fn read(path: &Path) -> Option<HostRecord> {
    HostRecord::decode(&fs::read_to_string(path).ok()?)
}

fn sleeper() -> Command {
    let mut command =
        Command::new(std::env::current_exe().expect("test executable should have a path"));
    command
        .args([
            "--exact",
            "command::tests::command_child_sleeps",
            "--ignored",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}
