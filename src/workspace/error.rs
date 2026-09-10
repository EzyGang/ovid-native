use crate::workspace::FileChange;

#[derive(Debug)]
pub(crate) enum WorkspaceError {
    Configuration(String),
    Path(String),
    Read(String),
    Encoding(String),
    Binary(String),
    Limit(String),
    ObservationNotFound(String),
    ObservationCollision(String),
    UnseenLine(String),
    ObservedLineChanged(String),
    Stale(String),
    EditMode(String),
    Patch(String),
    PartialCommit {
        landed: Vec<String>,
        pending: Vec<String>,
        changes: Vec<FileChange>,
    },
    Write(String),
    Cancelled,
    Closed,
    Deadline,
}

impl WorkspaceError {
    #[must_use]
    pub(crate) fn into_message(self) -> String {
        match self {
            Self::Configuration(message)
            | Self::Path(message)
            | Self::Read(message)
            | Self::Encoding(message)
            | Self::Binary(message)
            | Self::Limit(message)
            | Self::ObservationNotFound(message)
            | Self::ObservationCollision(message)
            | Self::UnseenLine(message)
            | Self::ObservedLineChanged(message)
            | Self::Stale(message)
            | Self::EditMode(message)
            | Self::Patch(message)
            | Self::Write(message) => message,
            Self::PartialCommit {
                landed, pending, ..
            } => format!(
                "workspace operation committed [{}] with pending [{}]",
                landed.join(", "),
                pending.join(", ")
            ),
            Self::Cancelled => "workspace operation was cancelled".to_owned(),
            Self::Closed => "workspace is closed".to_owned(),
            Self::Deadline => "workspace operation reached its deadline".to_owned(),
        }
    }
}
