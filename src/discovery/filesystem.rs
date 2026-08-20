use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::discovery::types::{DiscoveryError, MAX_TEXT_FILES, validate_entry_name};
use crate::workspace::{WorkControl, WorkStopped};

pub(crate) fn find_ancestor_entry(
    start: &Path,
    name: &str,
) -> Result<Option<PathBuf>, DiscoveryError> {
    validate_entry_name(name)?;
    let mut current = absolute_directory(start, "discovery start")?;

    loop {
        match fs::symlink_metadata(current.join(name)) {
            Ok(_) => return Ok(Some(current)),
            Err(error) if error.kind() == ErrorKind::NotFound => (),
            Err(error) => {
                return Err(DiscoveryError::Read(format!(
                    "could not inspect ancestor entry {name}: {error}"
                )));
            }
        }
        if !current.pop() {
            return Ok(None);
        }
    }
}

pub(crate) fn read_text_files(
    paths: Vec<String>,
    control: &WorkControl,
) -> Result<Vec<(String, String)>, DiscoveryError> {
    if paths.len() > MAX_TEXT_FILES {
        return Err(DiscoveryError::Configuration(format!(
            "text file batch must contain at most {MAX_TEXT_FILES} paths"
        )));
    }

    let mut files = Vec::with_capacity(paths.len());
    for path in paths {
        control.checkpoint().map_err(stopped_to_error)?;
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(DiscoveryError::Read(format!(
                    "could not read text file {path}: {error}"
                )));
            }
        };
        let content = String::from_utf8(bytes).map_err(|error| {
            DiscoveryError::Encoding(format!("text file is not valid UTF-8: {path}: {error}"))
        })?;
        let content = if content.contains('\r') {
            content.replace("\r\n", "\n").replace('\r', "\n")
        } else {
            content
        };
        files.push((path, content));
    }

    Ok(files)
}

fn stopped_to_error(stopped: WorkStopped) -> DiscoveryError {
    match stopped {
        WorkStopped::Cancelled => DiscoveryError::Cancelled,
        WorkStopped::Deadline => {
            DiscoveryError::Read("text file read reached its deadline".to_owned())
        }
    }
}

fn absolute_directory(path: &Path, label: &str) -> Result<PathBuf, DiscoveryError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| {
                DiscoveryError::Path(format!("cannot resolve current directory: {error}"))
            })?
            .join(path)
    };
    if !fs::metadata(&absolute).is_ok_and(|metadata| metadata.is_dir()) {
        return Err(DiscoveryError::Path(format!("{label} must be a directory")));
    }

    Ok(absolute)
}
