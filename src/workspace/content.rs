use std::fs::File;
use std::io::{Read, Take};
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
