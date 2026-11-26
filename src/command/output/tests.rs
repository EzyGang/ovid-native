use std::io::{self, Read};

use crate::command::CommandError;
use crate::command::output::{CapturedOutput, capture};

struct FailingReader;

impl Read for FailingReader {
    fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("pipe failed"))
    }
}

#[test]
fn capture_keeps_head_and_tail_of_large_output() {
    let printed: Vec<u8> = (0..200 * 1024)
        .map(|index| b'a' + (index % 26) as u8)
        .collect();

    let output = capture(printed.as_slice(), 16_384, 65_536).expect("capture should succeed");

    assert_eq!(output.head.as_bytes(), &printed[..16_384]);
    assert_eq!(output.tail.as_bytes(), &printed[printed.len() - 49_152..]);
    assert_eq!(output.omitted_bytes, 200 * 1024 - 65_536);
}

#[test]
fn capture_without_head_keeps_the_tail_only() {
    let output = capture(&b"1234567890123"[..], 0, 10).expect("capture should succeed");

    assert_eq!(output, captured("", "4567890123", 3));
}

#[test]
fn capture_with_head_budget_only_keeps_the_head() {
    let output = capture(&b"1234567890123"[..], 10, 10).expect("capture should succeed");

    assert_eq!(output, captured("1234567890", "", 3));
}

#[test]
fn capture_returns_small_output_without_duplicates() {
    assert_eq!(
        capture(&b"abcdef"[..], 4, 10).expect("capture should succeed"),
        captured("abcd", "ef", 0)
    );
    assert_eq!(
        capture(&b"ab"[..], 4, 10).expect("capture should succeed"),
        captured("ab", "", 0)
    );
}

#[test]
fn capture_never_splits_multibyte_characters_at_either_cut() {
    for character in ['é', '€', '😀'] {
        let width = character.len_utf8();
        let printed = format!("a{character}xxxxxxxx{character}b");

        let cut = capture(printed.as_bytes(), 2, 4).expect("capture should succeed");
        let whole_head =
            capture(printed.as_bytes(), 1 + width, 3 + width).expect("capture should succeed");
        let fits =
            capture(format!("a{character}").as_bytes(), 2, 10).expect("capture should succeed");

        let omitted = u64::try_from(printed.len() - 2).expect("length should fit");
        assert_eq!(cut, captured("a", "b", omitted));
        let omitted = u64::try_from(printed.len() - 2 - width).expect("length should fit");
        assert_eq!(whole_head, captured(&format!("a{character}"), "b", omitted));
        assert_eq!(fits, captured("a", &character.to_string(), 0));
    }
}

#[test]
fn capture_decodes_invalid_printed_bytes_lossily() {
    assert_eq!(
        capture(&[b'a', 0xFF, b'b'][..], 3, 3).expect("capture should succeed"),
        captured("a\u{FFFD}b", "", 0)
    );
    assert_eq!(
        capture(&[0x80; 6][..], 6, 6).expect("capture should succeed"),
        captured(&"\u{FFFD}".repeat(6), "", 0)
    );
}

#[test]
fn capture_reports_read_failures() {
    let result = capture(FailingReader, 0, 10);

    assert!(
        matches!(&result, Err(CommandError::Output(message)) if message == "pipe failed"),
        "{result:?}"
    );
}

fn captured(head: &str, tail: &str, omitted_bytes: u64) -> CapturedOutput {
    CapturedOutput {
        head: head.to_owned(),
        tail: tail.to_owned(),
        omitted_bytes,
    }
}
