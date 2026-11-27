use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use brush_core::{
    ExecutionContext,
    commands::{ProgramEnvironment, compose_launch_request},
    extensions::ProcessLauncher,
    openfiles::{self, OpenFiles},
};
use ovid_command_process::ProcessScope;
use tempfile::TempDir;
use tokio::time;

use super::{COMMAND_RUNTIME, create_shell};

#[test]
fn launcher_marks_children_as_native_processes() {
    let runtime = COMMAND_RUNTIME.as_ref().expect("runtime should start");
    let temporary = TempDir::new().expect("temporary directory should exist");
    let scope = Arc::new(ProcessScope::default());

    runtime.block_on(async {
        let mut shell = create_shell(temporary.path(), &[], Arc::clone(&scope))
            .await
            .expect("shell should start");
        let mut params = shell.default_exec_params();
        params.set_fd(
            OpenFiles::STDOUT_FD,
            openfiles::null().expect("null file should open"),
        );
        params.set_fd(
            OpenFiles::STDERR_FD,
            openfiles::null().expect("null file should open"),
        );
        let context = ExecutionContext::new(&mut shell, "sleeper".to_owned(), params);
        let executable = std::env::current_exe().expect("test executable should have a path");
        let request = compose_launch_request(
            &context,
            &executable,
            "sleeper",
            &SLEEPER_ARGUMENTS,
            ProgramEnvironment::Exported,
        );

        let child = context
            .shell
            .process_launcher()
            .spawn(request)
            .expect("child should start");

        let pid = child.pid().expect("launcher should report the process ID");
        assert!(pid > 0);
        #[cfg(unix)]
        assert_eq!(child.pgid(), Some(pid));
        #[cfg(windows)]
        assert_eq!(child.pgid(), None);
        scope
            .terminate(Duration::ZERO)
            .expect("scope should terminate");
    });
}

#[test]
fn scope_terminate_stops_children_and_refuses_later_spawns() {
    let runtime = COMMAND_RUNTIME.as_ref().expect("runtime should start");
    let scope = ProcessScope::default();

    runtime.block_on(async {
        let (mut child, _) = scope.spawn(sleeper(), false).expect("child should start");
        let waiter = tokio::spawn(async move { child.wait().await });
        let started = Instant::now();

        scope
            .terminate(Duration::from_secs(5))
            .expect("scope should terminate");
        time::timeout(Duration::from_secs(5), waiter)
            .await
            .expect("child should stop")
            .expect("waiter should join")
            .expect("child status should be available");

        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(scope.spawn(sleeper(), false).is_err());
    });
}

#[test]
fn scope_drop_stops_children() {
    let runtime = COMMAND_RUNTIME.as_ref().expect("runtime should start");
    let scope = ProcessScope::default();

    runtime.block_on(async {
        let (mut child, _) = scope.spawn(sleeper(), false).expect("child should start");
        let waiter = tokio::spawn(async move { child.wait().await });

        drop(scope);

        time::timeout(Duration::from_secs(5), waiter)
            .await
            .expect("child should stop")
            .expect("waiter should join")
            .expect("child status should be available");
    });
}

#[cfg(unix)]
#[test]
fn scope_keeps_children_in_one_group_apart_from_the_host() {
    let runtime = COMMAND_RUNTIME.as_ref().expect("runtime should start");
    let scope = ProcessScope::default();

    runtime.block_on(async {
        let (first, first_group) = scope.spawn(sleeper(), false).expect("child should start");
        let (second, second_group) = scope.spawn(sleeper(), false).expect("child should start");
        let first_pid = first.id().expect("first child should have an ID");
        let second_pid = second.id().expect("second child should have an ID");

        assert_eq!(
            first_group,
            Some(i32::try_from(first_pid).expect("ID should fit"))
        );
        assert_eq!(second_group, first_group);
        assert_eq!(process_group(first_pid), first_group);
        assert_eq!(process_group(second_pid), first_group);
        assert_ne!(process_group(std::process::id()), first_group);
        scope
            .terminate(Duration::ZERO)
            .expect("scope should terminate");
    });
}

const SLEEPER_ARGUMENTS: [&str; 3] = [
    "--exact",
    "command::tests::command_child_sleeps",
    "--ignored",
];

fn sleeper() -> Command {
    let mut command =
        Command::new(std::env::current_exe().expect("test executable should have a path"));
    command
        .args(SLEEPER_ARGUMENTS)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

#[cfg(unix)]
fn process_group(pid: u32) -> Option<i32> {
    let output = Command::new("ps")
        .args(["-o", "pgid=", "-p", &pid.to_string()])
        .output()
        .expect("ps should run");
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}
