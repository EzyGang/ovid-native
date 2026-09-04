use std::fs;
use std::thread;
use std::time::{Duration, Instant};

use tempfile::TempDir;

use super::{CommandCancellation, CommandError, CommandRequest, execute};
use crate::workspace::Workspace;

#[test]
fn command_runs_in_workspace_with_environment_and_merged_output() {
    let temporary = TempDir::new().expect("temporary directory should exist");
    fs::create_dir(temporary.path().join("nested")).expect("nested directory should exist");
    let workspace = Workspace::new(temporary.path().to_str().expect("path should be UTF-8"))
        .expect("workspace should exist");
    let mut request = request("printf '%s|%s' \"$CUSTOM\" \"$PWD\"; printf '|error' >&2; exit 7");
    request.cwd = Some("nested".to_owned());
    request.env = vec![("CUSTOM".to_owned(), "value".to_owned())];

    let result = execute(&workspace, request).expect("command should run");

    assert_eq!(
        result.output,
        format!(
            "value|{}|error",
            dunce::simplified(workspace.root()).join("nested").display()
        )
    );
    assert_eq!(result.exit_code, Some(7));
    assert!(!result.timed_out);
    assert!(!result.truncated);
}

#[test]
fn command_uses_embedded_bash_parsing_without_external_programs() {
    let temporary = TempDir::new().expect("temporary directory should exist");
    let workspace = Workspace::new(temporary.path().to_str().expect("path should be UTF-8"))
        .expect("workspace should exist");
    let mut request = request(
        "values=(one two); total=$((2 + 3)); \
         for value in \"${values[@]}\"; do printf '%s-' \"$value\"; done; \
         false || printf 'recovered-%s\\n' \"$total\"",
    );
    request.env = vec![("PATH".to_owned(), String::new())];

    let result = execute(&workspace, request).expect("embedded command should run");

    assert_eq!(result.output, "one-two-recovered-5\n");
    assert_eq!(result.exit_code, Some(0));
}

#[test]
fn command_handles_control_flow_and_redirection_inside_workspace() {
    let temporary = TempDir::new().expect("temporary directory should exist");
    let workspace = Workspace::new(temporary.path().to_str().expect("path should be UTF-8"))
        .expect("workspace should exist");

    let result = execute(
        &workspace,
        request("for value in alpha beta; do printf '%s\\n' \"$value\"; done > values.txt"),
    )
    .expect("redirection should run");

    assert_eq!(result.exit_code, Some(0));
    assert_eq!(
        fs::read_to_string(workspace.root().join("values.txt"))
            .expect("redirected file should exist"),
        "alpha\nbeta\n"
    );
}

#[test]
fn command_accepts_contained_absolute_cwd_and_bounds_output() {
    let temporary = TempDir::new().expect("temporary directory should exist");
    let workspace = Workspace::new(temporary.path().to_str().expect("path should be UTF-8"))
        .expect("workspace should exist");
    let mut request = request("printf 1234567890123");
    request.cwd = Some(temporary.path().display().to_string());
    request.max_output_bytes = 10;

    let result = execute(&workspace, request).expect("command should run");

    assert_eq!(result.output, "4567890123");
    assert!(result.truncated);
    assert_eq!(result.exit_code, Some(0));
}

#[test]
#[ignore = "helper process for command cancellation tests"]
fn command_child_sleeps() {
    thread::sleep(Duration::from_secs(30));
}

fn sleeping_command() -> String {
    let executable = std::env::current_exe().expect("test executable should have a path");
    let escaped = executable.display().to_string().replace('\'', "'\\''");
    format!("'{escaped}' --exact command::tests::command_child_sleeps --ignored")
}

#[test]
fn command_timeout_kills_the_process_group() {
    let temporary = TempDir::new().expect("temporary directory should exist");
    let workspace = Workspace::new(temporary.path().to_str().expect("path should be UTF-8"))
        .expect("workspace should exist");
    let mut request = request(&sleeping_command());
    request.timeout = Duration::from_secs(1);
    let started = Instant::now();

    let result = execute(&workspace, request).expect("timeout should return a result");

    assert!(result.timed_out, "{result:?}");
    assert_eq!(result.exit_code, None);
    assert!(started.elapsed() < Duration::from_secs(4));
}

#[test]
fn command_cancellation_kills_the_process_group() {
    let temporary = TempDir::new().expect("temporary directory should exist");
    let workspace = Workspace::new(temporary.path().to_str().expect("path should be UTF-8"))
        .expect("workspace should exist");
    let request = request(&sleeping_command());
    let cancellation = request.cancellation.clone();
    let worker = thread::spawn(move || execute(&workspace, request));
    thread::sleep(Duration::from_millis(50));

    cancellation.cancel();
    let result = worker.join().expect("command worker should join");

    assert!(matches!(result, Err(CommandError::Cancelled)), "{result:?}");
}

#[test]
fn command_rejects_invalid_configuration_and_paths() {
    let temporary = TempDir::new().expect("temporary directory should exist");
    let workspace = Workspace::new(temporary.path().to_str().expect("path should be UTF-8"))
        .expect("workspace should exist");
    let outside = TempDir::new().expect("outside directory should exist");

    let mut invalid_timeout = request("true");
    invalid_timeout.timeout = Duration::ZERO;
    assert!(matches!(
        execute(&workspace, invalid_timeout),
        Err(CommandError::Configuration(_))
    ));

    let mut invalid_environment = request("true");
    invalid_environment.env = vec![("BAD=NAME".to_owned(), "value".to_owned())];
    assert!(matches!(
        execute(&workspace, invalid_environment),
        Err(CommandError::Configuration(_))
    ));

    let mut escaped = request("true");
    escaped.cwd = Some(outside.path().display().to_string());
    assert!(matches!(
        execute(&workspace, escaped),
        Err(CommandError::Path(_))
    ));

    workspace.close();
    assert!(matches!(
        execute(&workspace, request("true")),
        Err(CommandError::Closed)
    ));
}

fn request(command: &str) -> CommandRequest {
    CommandRequest {
        command: command.to_owned(),
        cwd: None,
        env: Vec::new(),
        timeout: Duration::from_secs(300),
        max_output_bytes: 65_536,
        cancellation: CommandCancellation::new(),
    }
}
