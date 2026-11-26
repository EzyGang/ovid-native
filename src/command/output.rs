#[cfg(test)]
mod tests;

use std::collections::VecDeque;
use std::io::Read;

use crate::command::CommandError;

/// Merged command output split into a leading head and a trailing tail.
///
/// `head` and `tail` never share a byte and never start or end inside a UTF-8 character that the
/// command printed. `omitted_bytes` counts every printed byte that neither part holds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CapturedOutput {
    pub head: String,
    pub tail: String,
    pub omitted_bytes: u64,
}

/// Reads `reader` to its end and keeps at most `output_limit` bytes, the first `head_limit` of them
/// in the head.
pub(crate) fn capture<R>(
    reader: R,
    head_limit: usize,
    output_limit: usize,
) -> Result<CapturedOutput, CommandError>
where
    R: Read,
{
    let (mut head, mut tail, total) =
        read_parts(reader, head_limit, output_limit.saturating_sub(head_limit))?;

    let end = head_end(&head);
    if total > output_limit.try_into().unwrap_or(u64::MAX) {
        head.truncate(end);
        let start = tail_start(tail.make_contiguous());
        tail.drain(..start);
    } else {
        for byte in head.drain(end..).rev() {
            tail.push_front(byte);
        }
    }
    let kept: u64 = (head.len() + tail.len()).try_into().unwrap_or(u64::MAX);
    Ok(CapturedOutput {
        head: String::from_utf8_lossy(&head).into_owned(),
        tail: String::from_utf8_lossy(tail.make_contiguous()).into_owned(),
        omitted_bytes: total.saturating_sub(kept),
    })
}

fn read_parts<R>(
    mut reader: R,
    head_limit: usize,
    tail_limit: usize,
) -> Result<(Vec<u8>, VecDeque<u8>, u64), CommandError>
where
    R: Read,
{
    let mut head = Vec::with_capacity(head_limit);
    let mut tail = VecDeque::with_capacity(tail_limit);
    let mut total = 0u64;
    let mut chunk = [0u8; 65_536];
    loop {
        let read = reader
            .read(&mut chunk)
            .map_err(|error| CommandError::Output(error.to_string()))?;
        if read == 0 {
            return Ok((head, tail, total));
        }
        total = total.saturating_add(read.try_into().unwrap_or(u64::MAX));
        let (head_bytes, tail_bytes) =
            chunk[..read].split_at(read.min(head_limit.saturating_sub(head.len())));
        head.extend_from_slice(head_bytes);
        let kept = &tail_bytes[tail_bytes.len().saturating_sub(tail_limit)..];
        tail.drain(..(tail.len() + kept.len()).saturating_sub(tail_limit));
        tail.extend(kept);
    }
}

fn head_end(bytes: &[u8]) -> usize {
    let Some((back, lead)) = bytes
        .iter()
        .rev()
        .take(4)
        .enumerate()
        .find(|(_, byte)| !is_continuation(**byte))
    else {
        return bytes.len();
    };
    let width = match lead {
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF7 => 4,
        _ => 1,
    };
    if width > back + 1 {
        return bytes.len() - back - 1;
    }
    bytes.len()
}

fn tail_start(bytes: &[u8]) -> usize {
    bytes
        .iter()
        .take(3)
        .take_while(|byte| is_continuation(**byte))
        .count()
}

const fn is_continuation(byte: u8) -> bool {
    byte & 0xC0 == 0x80
}
