use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};

use crate::command::output::{head_end, tail_start};

/// Bytes that all output rings of one registry may hold together.
pub(super) struct BufferBudget {
    limit: usize,
    used: Mutex<usize>,
}

/// Merged job output with a fixed head and a rolling tail.
///
/// Offsets count every byte that the job printed and only grow. The head keeps the first bytes
/// up to a UTF-8 character boundary. The tail drops its oldest bytes when the ring or the shared
/// budget is full. The head never drops a byte.
pub(super) struct OutputRing {
    head: Vec<u8>,
    head_limit: usize,
    sealed: bool,
    tail: VecDeque<u8>,
    dropped: u64,
    capacity: usize,
    budget: Arc<BufferBudget>,
    held: usize,
}

/// Output from one offset to the end of a ring.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct RingRead {
    pub head: Vec<u8>,
    pub tail: Vec<u8>,
    pub omitted_bytes: u64,
    pub next_offset: u64,
    pub truncated_before: u64,
}

impl BufferBudget {
    pub(super) const fn new(limit: usize) -> Self {
        Self {
            limit,
            used: Mutex::new(0),
        }
    }

    /// Changes a holder from `held` to at most `wanted` bytes, never below `floor`, and returns
    /// the size that the holder may keep.
    fn resize(&self, held: usize, floor: usize, wanted: usize) -> usize {
        let mut used = self.used.lock().unwrap_or_else(PoisonError::into_inner);
        let others = used.saturating_sub(held);
        let granted = wanted.min(self.limit.saturating_sub(others)).max(floor);
        *used = others + granted;
        granted
    }
}

impl OutputRing {
    pub(super) fn new(head_limit: usize, capacity: usize, budget: Arc<BufferBudget>) -> Self {
        Self {
            head: Vec::new(),
            head_limit: head_limit.min(capacity),
            sealed: false,
            tail: VecDeque::new(),
            dropped: 0,
            capacity,
            budget,
            held: 0,
        }
    }

    pub(super) fn push(&mut self, bytes: &[u8]) {
        let mut rest = bytes;
        if !self.sealed {
            let room = self.head_limit - self.head.len();
            let (front, back) = bytes.split_at(room.min(bytes.len()));
            self.head.extend_from_slice(front);
            rest = back;
            if self.head.len() == self.head_limit {
                self.sealed = true;
                let end = head_end(&self.head);
                self.tail.extend(self.head.drain(end..));
            }
        }
        self.tail.extend(rest);

        let wanted = (self.head.len() + self.tail.len()).min(self.capacity);
        self.held = self.budget.resize(self.held, self.head.len(), wanted);
        let excess = self.tail.len() - (self.held - self.head.len());
        self.tail.drain(..excess);
        self.dropped += excess as u64;
    }

    /// Returns the output from `offset` to the end.
    ///
    /// While `complete` is false, the read stops before a trailing incomplete UTF-8 character, so
    /// the next read starts with that whole character.
    pub(super) fn read(&self, offset: u64, complete: bool) -> RingRead {
        let head_length = self.head.len() as u64;
        let tail_offset = head_length + self.dropped;
        let end = tail_offset + self.tail.len() as u64;
        let offset = offset.min(end);
        let mut head = self.head[offset.min(head_length) as usize..].to_vec();
        let skipped = (offset.max(tail_offset) - tail_offset) as usize;
        let mut tail: Vec<u8> = self.tail.range(skipped..).copied().collect();
        let mut omitted_bytes = tail_offset.saturating_sub(offset.max(head_length));
        if omitted_bytes > 0 {
            let cut = tail_start(&tail);
            tail.drain(..cut);
            omitted_bytes += cut as u64;
        }

        let mut next_offset = end;
        if !complete {
            let part = if tail.is_empty() {
                &mut head
            } else {
                &mut tail
            };
            let kept = head_end(part);
            next_offset -= (part.len() - kept) as u64;
            part.truncate(kept);
        }
        RingRead {
            head,
            tail,
            omitted_bytes,
            next_offset,
            truncated_before: self.dropped,
        }
    }
}

impl Drop for OutputRing {
    fn drop(&mut self) {
        self.budget.resize(self.held, 0, 0);
    }
}

/// Decodes output bytes lossily and removes ANSI escape sequences when `strip` is true.
pub(super) fn render(bytes: &[u8], strip: bool) -> String {
    let text = String::from_utf8_lossy(bytes);
    if strip {
        return strip_ansi(&text);
    }
    text.into_owned()
}

/// Removes ANSI escape sequences: CSI and OSC sequences and two-character escapes.
pub(super) fn strip_ansi(text: &str) -> String {
    let mut stripped = String::with_capacity(text.len());
    let mut characters = text.chars();
    while let Some(character) = characters.next() {
        if character != '\u{1b}' {
            stripped.push(character);
            continue;
        }
        match characters.next() {
            Some('[') => {
                let _ = characters.find(|next| ('@'..='~').contains(next));
            }
            Some(']') => {
                let mut previous = '\0';
                let _ = characters.find(|&next| {
                    let ended = next == '\u{7}' || (previous == '\u{1b}' && next == '\\');
                    previous = next;
                    ended
                });
            }
            _ => (),
        }
    }
    stripped
}
