use crate::workspace::WorkCompletion;

pub(crate) const MAX_DEPTH: usize = 64;
pub(crate) const MAX_RESULTS: usize = 10_000;
pub(crate) const MAX_TEXT_FILES: usize = 10_000;

#[derive(Debug)]
pub(crate) enum DiscoveryError {
    Configuration(String),
    Path(String),
    Read(String),
    Encoding(String),
    Cancelled,
}

#[derive(Clone, Debug)]
pub(crate) struct NamedFileRequest {
    pub filename: String,
    pub max_depth: usize,
    pub limit: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct NamedFileResult {
    pub paths: Vec<String>,
    pub completion: WorkCompletion,
}

pub(crate) fn validate_entry_name(name: &str) -> Result<(), DiscoveryError> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', '\0']) {
        return Err(DiscoveryError::Configuration(
            "entry name must be one file name".to_owned(),
        ));
    }

    Ok(())
}
