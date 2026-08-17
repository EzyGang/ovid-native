use std::fs::File;
use std::io::{Read, Take};
use std::ops::Range;
use std::path::Path;

use crate::workspace::WorkspaceError;
use crate::workspace::control::{WorkControl, WorkStopped};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReadExtent {
    Complete { max_bytes: u64 },
    Prefix { max_bytes: u64 },
}

#[derive(Clone, Debug)]
pub(crate) struct ContentRead {
    pub bytes: Vec<u8>,
    pub searched_bytes: u64,
    pub total_bytes: u64,
    pub complete: bool,
    pub binary: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LineEnding {
    Lf,
    CrLf,
    Cr,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TextSerialization {
    pub bom: bool,
    pub line_ending: LineEnding,
}

#[derive(Clone, Debug)]
pub(crate) struct NormalizedText {
    pub source: String,
    pub serialization: TextSerialization,
    line_bounds: Vec<Range<usize>>,
}

impl NormalizedText {
    pub(crate) fn decode(bytes: Vec<u8>) -> Result<Self, WorkspaceError> {
        if bytes.contains(&0) {
            return Err(WorkspaceError::Binary(
                "workspace file contains binary content".to_owned(),
            ));
        }

        let bom = bytes.starts_with(&[0xef, 0xbb, 0xbf]);
        let source_bytes = if bom { &bytes[3..] } else { &bytes };
        let decoded = std::str::from_utf8(source_bytes).map_err(|_| {
            WorkspaceError::Encoding("workspace file is not valid UTF-8".to_owned())
        })?;
        let line_ending = detect_line_ending(decoded);
        let source = decoded.replace("\r\n", "\n").replace('\r', "\n");
        let line_bounds = line_bounds(&source);

        Ok(Self {
            source,
            serialization: TextSerialization { bom, line_ending },
            line_bounds,
        })
    }

    pub(crate) fn from_replacement(source: &str) -> Self {
        let line_ending = detect_line_ending(source);
        let bom = source.starts_with('\u{feff}');
        let without_bom = source.strip_prefix('\u{feff}').unwrap_or(source);
        let normalized = without_bom.replace("\r\n", "\n").replace('\r', "\n");
        let line_bounds = line_bounds(&normalized);

        Self {
            source: normalized,
            serialization: TextSerialization { bom, line_ending },
            line_bounds,
        }
    }

    pub(crate) fn decode_prefix(mut bytes: Vec<u8>) -> Result<Self, WorkspaceError> {
        if bytes.contains(&0) {
            return Err(WorkspaceError::Binary(
                "workspace file contains binary content".to_owned(),
            ));
        }
        if let Err(error) = std::str::from_utf8(&bytes) {
            if error.error_len().is_some() {
                return Err(WorkspaceError::Encoding(
                    "workspace file is not valid UTF-8".to_owned(),
                ));
            }
            bytes.truncate(error.valid_up_to());
        }

        Self::decode(bytes)
    }

    pub(crate) fn total_lines(&self) -> usize {
        self.line_bounds.len()
    }

    pub(crate) fn line(&self, number: usize) -> Option<&str> {
        number
            .checked_sub(1)
            .and_then(|index| self.line_bounds.get(index))
            .map(|bounds| &self.source[bounds.clone()])
    }

    pub(crate) fn line_bounds(&self, number: usize) -> Option<Range<usize>> {
        number
            .checked_sub(1)
            .and_then(|index| self.line_bounds.get(index))
            .cloned()
    }

    pub(crate) fn serialized_line_bounds(&self, number: usize) -> Option<Range<usize>> {
        let mut bounds = self.line_bounds(number)?;
        let bom_bytes = usize::from(self.serialization.bom) * 3;
        let line_ending_bytes = match self.serialization.line_ending {
            LineEnding::CrLf => number.checked_sub(1)?,
            LineEnding::Lf | LineEnding::Cr => 0,
        };
        let offset = bom_bytes.saturating_add(line_ending_bytes);
        bounds.start = bounds.start.saturating_add(offset);
        bounds.end = bounds.end.saturating_add(offset);
        Some(bounds)
    }

    pub(crate) fn serialize(&self) -> Vec<u8> {
        serialize_source(&self.source, &self.serialization)
    }

    pub(crate) fn serialize_with_current(&self, source: &str) -> Vec<u8> {
        serialize_source(source, &self.serialization)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WorkspaceTextSerialization {
    pub bom: bool,
    pub line_ending: LineEnding,
    pub terminal_newline: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct WorkspaceFileRead {
    pub path: String,
    pub observation: Option<crate::workspace::ObservationReceipt>,
    pub lines: Vec<crate::workspace::RenderedLine>,
    pub total_lines: usize,
    pub complete_presentation: bool,
    pub editable: bool,
    pub total_bytes: u64,
    pub observation_limit: u64,
    pub serialization: Option<WorkspaceTextSerialization>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WorkspaceDirectoryEntry {
    pub path: String,
    pub kind: String,
    pub size: Option<u64>,
}

#[derive(Clone, Debug)]
pub(crate) struct WorkspaceDirectoryRead {
    pub path: String,
    pub entries: Vec<WorkspaceDirectoryEntry>,
    pub truncated: bool,
}

pub(crate) fn read_content(
    path: &Path,
    extent: ReadExtent,
    control: &WorkControl,
) -> Result<ContentRead, WorkspaceError> {
    control.checkpoint().map_err(stopped_error)?;

    let file = File::open(path)
        .map_err(|error| WorkspaceError::Read(format!("cannot open file: {error}")))?;

    let total_bytes = file
        .metadata()
        .map_err(|error| WorkspaceError::Read(format!("cannot inspect file: {error}")))?
        .len();
    let max_bytes = match extent {
        ReadExtent::Complete { max_bytes } | ReadExtent::Prefix { max_bytes } => max_bytes,
    };
    let read_bytes = total_bytes.min(max_bytes);
    let mut bytes = Vec::with_capacity(usize::try_from(read_bytes).unwrap_or(usize::MAX));
    read_chunks(file.take(read_bytes), &mut bytes, control)?;
    let complete = total_bytes <= max_bytes;

    Ok(ContentRead {
        searched_bytes: bytes.len() as u64,
        binary: bytes.contains(&0),
        bytes,
        total_bytes,
        complete,
    })
}
pub(crate) fn inspect_text(path: &Path) -> Result<usize, WorkspaceError> {
    let mut file = File::open(path)
        .map_err(|error| WorkspaceError::Read(format!("cannot open file: {error}")))?;
    let mut buffer = [0_u8; 64 * 1024];
    let mut carry = Vec::with_capacity(4);
    let mut line_breaks = 0_usize;
    let mut any = false;
    let mut previous_cr = false;
    let mut ends_with_break = false;

    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| WorkspaceError::Read(format!("cannot read file: {error}")))?;
        if count == 0 {
            break;
        }
        any = true;
        if buffer[..count].contains(&0) {
            return Err(WorkspaceError::Binary(
                "workspace file contains binary content".to_owned(),
            ));
        }
        carry.extend_from_slice(&buffer[..count]);
        let valid_length = match std::str::from_utf8(&carry) {
            Ok(_) => carry.len(),
            Err(error) if error.error_len().is_none() => error.valid_up_to(),
            Err(_) => {
                return Err(WorkspaceError::Encoding(
                    "workspace file is not valid UTF-8".to_owned(),
                ));
            }
        };
        let valid = &carry[..valid_length];
        for byte in valid {
            match *byte {
                b'\n' => {
                    if !previous_cr {
                        line_breaks = line_breaks.saturating_add(1);
                    }
                    previous_cr = false;
                    ends_with_break = true;
                }
                b'\r' => {
                    line_breaks = line_breaks.saturating_add(1);
                    previous_cr = true;
                    ends_with_break = true;
                }
                _ => {
                    previous_cr = false;
                    ends_with_break = false;
                }
            }
        }
        carry.drain(..valid_length);
    }
    if !carry.is_empty() {
        return Err(WorkspaceError::Encoding(
            "workspace file is not valid UTF-8".to_owned(),
        ));
    }

    Ok(if any && !ends_with_break {
        line_breaks.saturating_add(1)
    } else {
        line_breaks
    })
}

fn read_chunks(
    mut reader: Take<File>,
    bytes: &mut Vec<u8>,
    control: &WorkControl,
) -> Result<(), WorkspaceError> {
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        control.checkpoint().map_err(stopped_error)?;
        let count = reader
            .read(&mut buffer)
            .map_err(|error| WorkspaceError::Read(format!("cannot read file: {error}")))?;
        if count == 0 {
            return Ok(());
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
}

fn stopped_error(stopped: WorkStopped) -> WorkspaceError {
    match stopped {
        WorkStopped::Cancelled => WorkspaceError::Cancelled,
        WorkStopped::Deadline => WorkspaceError::Deadline,
    }
}

fn detect_line_ending(source: &str) -> LineEnding {
    if source.contains("\r\n") {
        LineEnding::CrLf
    } else if source.contains('\r') {
        LineEnding::Cr
    } else {
        LineEnding::Lf
    }
}

fn line_bounds(source: &str) -> Vec<Range<usize>> {
    if source.is_empty() {
        return Vec::new();
    }

    let logical_end = source
        .strip_suffix('\n')
        .map_or(source.len(), |without_newline| without_newline.len());
    if logical_end == 0 {
        return std::iter::once(Range { start: 0, end: 0 }).collect();
    }

    let mut bounds = Vec::new();
    let mut start = 0;
    for (index, byte) in source.as_bytes()[..logical_end].iter().enumerate() {
        if *byte == b'\n' {
            bounds.push(start..index);
            start = index + 1;
        }
    }
    bounds.push(start..logical_end);
    bounds
}

fn serialize_source(source: &str, serialization: &TextSerialization) -> Vec<u8> {
    let line_ending = match serialization.line_ending {
        LineEnding::Lf => "\n",
        LineEnding::CrLf => "\r\n",
        LineEnding::Cr => "\r",
    };
    let encoded = if line_ending == "\n" {
        source.to_owned()
    } else {
        source.replace('\n', line_ending)
    };
    let mut bytes = Vec::with_capacity(encoded.len() + usize::from(serialization.bom) * 3);
    if serialization.bom {
        bytes.extend_from_slice(&[0xef, 0xbb, 0xbf]);
    }
    bytes.extend_from_slice(encoded.as_bytes());
    bytes
}
