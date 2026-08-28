use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::workspace::control::{WorkControl, WorkStopped};
use crate::workspace::{LineRange, WorkspaceError};

#[derive(Debug)]
pub(crate) struct SelectedTextRead {
    pub lines: Vec<(usize, String)>,
    pub total_lines: usize,
    pub total_bytes: u64,
}

pub(crate) fn read_selected_text(
    path: &Path,
    ranges: &[LineRange],
    max_bytes: u64,
    control: &WorkControl,
) -> Result<SelectedTextRead, WorkspaceError> {
    control.checkpoint().map_err(stopped_error)?;
    let mut file = File::open(path)
        .map_err(|error| WorkspaceError::Read(format!("cannot open file: {error}")))?;
    let total_bytes = file
        .metadata()
        .map_err(|error| WorkspaceError::Read(format!("cannot inspect file: {error}")))?
        .len();
    let mut scanner = SelectedTextScanner::new(ranges, max_bytes);
    let mut validation_carry = Vec::with_capacity(4);
    let mut buffer = [0_u8; 64 * 1024];

    loop {
        control.checkpoint().map_err(stopped_error)?;
        let count = file
            .read(&mut buffer)
            .map_err(|error| WorkspaceError::Read(format!("cannot read file: {error}")))?;
        if count == 0 {
            break;
        }
        let chunk = &buffer[..count];
        if chunk.contains(&0) {
            return Err(WorkspaceError::Binary(
                "workspace file contains binary content".to_owned(),
            ));
        }
        validate_utf8_chunk(&mut validation_carry, chunk)?;
        scanner.push(chunk)?;
    }
    if !validation_carry.is_empty() {
        return Err(WorkspaceError::Encoding(
            "workspace file is not valid UTF-8".to_owned(),
        ));
    }
    scanner.finish()?;

    Ok(SelectedTextRead {
        lines: scanner.lines,
        total_lines: scanner.line_number.saturating_sub(1),
        total_bytes,
    })
}

struct SelectedTextScanner<'a> {
    ranges: &'a [LineRange],
    range_index: usize,
    line_number: usize,
    lines: Vec<(usize, String)>,
    current: Vec<u8>,
    remaining_bytes: u64,
    output_full: bool,
    saw_any: bool,
    ends_with_break: bool,
    previous_cr: bool,
}

impl<'a> SelectedTextScanner<'a> {
    fn new(ranges: &'a [LineRange], max_bytes: u64) -> Self {
        Self {
            ranges,
            range_index: 0,
            line_number: 1,
            lines: Vec::new(),
            current: Vec::new(),
            remaining_bytes: max_bytes,
            output_full: false,
            saw_any: false,
            ends_with_break: false,
            previous_cr: false,
        }
    }

    fn push(&mut self, chunk: &[u8]) -> Result<(), WorkspaceError> {
        for byte in chunk {
            self.saw_any = true;
            if self.previous_cr {
                self.previous_cr = false;
                if *byte == b'\n' {
                    self.ends_with_break = true;
                    continue;
                }
            }
            match *byte {
                b'\r' => {
                    self.finish_line()?;
                    self.previous_cr = true;
                    self.ends_with_break = true;
                }
                b'\n' => {
                    self.finish_line()?;
                    self.ends_with_break = true;
                }
                value => {
                    self.ends_with_break = false;
                    self.push_line_byte(value);
                }
            }
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<(), WorkspaceError> {
        if self.saw_any && !self.ends_with_break {
            self.finish_line()?;
        }
        Ok(())
    }

    fn finish_line(&mut self) -> Result<(), WorkspaceError> {
        if self.current_selected() && !self.output_full {
            let source = std::str::from_utf8(&self.current).map_err(|_| {
                WorkspaceError::Encoding("workspace file is not valid UTF-8".to_owned())
            })?;
            let source = if self.line_number == 1 {
                source.strip_prefix('\u{feff}').unwrap_or(source)
            } else {
                source
            };
            if source.len() as u64 > self.remaining_bytes {
                self.output_full = true;
            } else {
                self.remaining_bytes = self.remaining_bytes.saturating_sub(source.len() as u64);
                self.lines.push((self.line_number, source.to_owned()));
            }
        }
        self.current.clear();
        self.line_number = self.line_number.saturating_add(1);
        while self.range_index < self.ranges.len()
            && self.ranges[self.range_index].end < self.line_number
        {
            self.range_index += 1;
        }
        Ok(())
    }

    fn push_line_byte(&mut self, value: u8) {
        if !self.current_selected() || self.output_full {
            return;
        }
        let bom_allowance = if self.line_number == 1 { 3 } else { 0 };
        if self.current.len() as u64 >= self.remaining_bytes.saturating_add(bom_allowance) {
            self.current.clear();
            self.output_full = true;
            return;
        }
        self.current.push(value);
    }

    fn current_selected(&self) -> bool {
        self.ranges
            .get(self.range_index)
            .is_some_and(|range| range.start <= self.line_number && self.line_number <= range.end)
    }
}

fn validate_utf8_chunk(carry: &mut Vec<u8>, chunk: &[u8]) -> Result<(), WorkspaceError> {
    carry.extend_from_slice(chunk);
    let valid_length = match std::str::from_utf8(carry) {
        Ok(_) => carry.len(),
        Err(error) if error.error_len().is_none() => error.valid_up_to(),
        Err(_) => {
            return Err(WorkspaceError::Encoding(
                "workspace file is not valid UTF-8".to_owned(),
            ));
        }
    };
    carry.drain(..valid_length);
    Ok(())
}

fn stopped_error(stopped: WorkStopped) -> WorkspaceError {
    match stopped {
        WorkStopped::Cancelled => WorkspaceError::Cancelled,
        WorkStopped::Deadline => WorkspaceError::Deadline,
    }
}
