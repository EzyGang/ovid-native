use std::sync::Arc;
use std::time::Duration;

use ovid_command_process::TerminalSize;

use crate::command::CommandError;
use crate::command::jobs::ring::{BufferBudget, OutputRing, RingRead, strip_ansi};
use crate::command::jobs::types::JOB_HEAD_BYTES;
use crate::command::jobs::{CommandJobLimits, CommandJobRequest, CommandJobSignal};

#[test]
fn ring_returns_only_bytes_after_the_offset() {
    let mut ring = ring(4, 16, 1_024);
    ring.push(b"abcdef");
    let first = ring.read(0, true);
    ring.push(b"gh");

    let second = ring.read(first.next_offset, true);

    assert_eq!(first, read("abcd", "ef", 0, 6, 0));
    assert_eq!(second, read("", "gh", 0, 8, 0));
    assert_eq!(ring.read(99, true), read("", "", 0, 8, 0));
}

#[test]
fn ring_keeps_the_head_and_drops_the_oldest_tail_bytes_on_overflow() {
    let mut ring = ring(4, 10, 1_024);
    ring.push(b"0123456789");
    ring.push(b"abcdef");

    assert_eq!(ring.read(0, true), read("0123", "abcdef", 6, 16, 6));
    assert_eq!(ring.read(2, true), read("23", "abcdef", 6, 16, 6));
    assert_eq!(ring.read(12, true), read("", "cdef", 0, 16, 6));
}

#[test]
fn ring_never_splits_characters_at_its_cuts() {
    for character in ['é', '€', '😀'] {
        let printed = format!("a{character}xxxx{character}b");
        let mut ring = ring(2, 4, 1_024);
        ring.push(printed.as_bytes());

        let cut = ring.read(0, true);
        let tail = String::from_utf8(cut.tail.clone()).expect("tail should hold whole characters");

        assert_eq!(cut.head, b"a", "{character}");
        assert!(
            printed.ends_with(&tail) && tail.ends_with('b'),
            "{character}"
        );
        assert_eq!(cut.next_offset, printed.len() as u64);
        assert_eq!(
            cut.omitted_bytes + (cut.head.len() + cut.tail.len()) as u64,
            printed.len() as u64
        );
    }
}

#[test]
fn ring_holds_back_a_trailing_partial_character_until_output_completes() {
    let mut ring = ring(0, 16, 1_024);
    let euro = "€".as_bytes();
    ring.push(&[b'a', euro[0], euro[1]]);

    let partial = ring.read(0, false);
    ring.push(&euro[2..]);

    assert_eq!(partial, read("", "a", 0, 1, 0));
    assert_eq!(
        ring.read(partial.next_offset, false),
        read("", "€", 0, 4, 0)
    );
}

#[test]
fn shared_budget_trims_tails_but_never_heads() {
    let budget = Arc::new(BufferBudget::new(16));
    let mut first = OutputRing::new(4, 16, Arc::clone(&budget));
    let mut second = OutputRing::new(4, 16, Arc::clone(&budget));
    let mut third = OutputRing::new(4, 16, Arc::clone(&budget));
    first.push(b"0123456789");
    second.push(b"abcdefghij");
    third.push(b"wxyz12");

    assert_eq!(first.read(0, true), read("0123", "456789", 0, 10, 0));
    assert_eq!(second.read(0, true), read("abcd", "ij", 4, 10, 4));
    assert_eq!(third.read(0, true), read("wxyz", "", 2, 6, 2));

    drop(first);
    second.push(b"k");
    assert_eq!(second.read(0, true).tail, "ijk".as_bytes());
}

#[test]
fn strip_ansi_removes_escape_sequences_only() {
    let text = "\u{1b}[1;31mred\u{1b}[0m \u{1b}]0;title\u{7}plain\u{1b}]8;;x\u{1b}\\ end\u{1b}=";

    assert_eq!(strip_ansi(text), "red plain end");
}

#[test]
fn job_request_validation_rejects_invalid_values() {
    let limits = CommandJobLimits::default();
    let valid = request();
    let cases = [
        CommandJobRequest {
            command: "a\0".to_owned(),
            ..request()
        },
        CommandJobRequest {
            timeout: Some(Duration::ZERO),
            ..request()
        },
        CommandJobRequest {
            max_buffer_bytes: JOB_HEAD_BYTES - 1,
            ..request()
        },
        CommandJobRequest {
            max_buffer_bytes: limits.buffer_limit + 1,
            ..request()
        },
        CommandJobRequest {
            terminal: Some(TerminalSize { cols: 0, rows: 5 }),
            ..request()
        },
    ];

    assert!(valid.validate(&limits).is_ok());
    for case in cases {
        assert!(matches!(
            case.validate(&limits),
            Err(CommandError::Configuration(_))
        ));
    }
}

#[test]
fn job_signal_parses_known_names() {
    assert_eq!(
        CommandJobSignal::parse("interrupt").ok(),
        Some(CommandJobSignal::Interrupt)
    );
    assert_eq!(
        CommandJobSignal::parse("terminate").ok(),
        Some(CommandJobSignal::Terminate)
    );
    assert_eq!(
        CommandJobSignal::parse("kill").ok(),
        Some(CommandJobSignal::Kill)
    );
    assert!(matches!(
        CommandJobSignal::parse("hup"),
        Err(CommandError::Configuration(_))
    ));
}

fn ring(head_limit: usize, capacity: usize, budget: usize) -> OutputRing {
    OutputRing::new(head_limit, capacity, Arc::new(BufferBudget::new(budget)))
}

fn read(
    head: &str,
    tail: &str,
    omitted_bytes: u64,
    next_offset: u64,
    truncated_before: u64,
) -> RingRead {
    RingRead {
        head: head.as_bytes().to_vec(),
        tail: tail.as_bytes().to_vec(),
        omitted_bytes,
        next_offset,
        truncated_before,
    }
}

fn request() -> CommandJobRequest {
    CommandJobRequest {
        command: "true".to_owned(),
        cwd: None,
        env: Vec::new(),
        timeout: None,
        terminal: None,
        max_buffer_bytes: 1_048_576,
    }
}
