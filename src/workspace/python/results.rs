use crate::workspace::{
    EditResult, FileChange, LineEnding, LineRange, ObservationReceipt, PostEditSource,
    RenderedLine, WorkspaceDirectoryEntry, WorkspaceDirectoryRead, WorkspaceDirectoryTreeLine,
    WorkspaceDirectoryTreeRead, WorkspaceFileRead, WorkspaceTextSerialization,
};

pub(super) type NativeObservationReceipt = (String, String, String, u64, Vec<(usize, usize)>, bool);
pub(super) type NativeRenderedLine = (usize, String, String);
pub(super) type NativeTextSerialization = (bool, String, bool);
pub(super) type NativeFileRead = (
    String,
    Option<NativeObservationReceipt>,
    Vec<NativeRenderedLine>,
    usize,
    bool,
    bool,
    u64,
    u64,
    Option<NativeTextSerialization>,
);
pub(super) type NativeDirectoryRead = (String, Vec<(String, String, Option<u64>)>, bool);
pub(super) type NativeDirectoryTreeLine = (usize, String, String, Option<usize>);
pub(super) type NativeDirectoryTreeRead = (
    String,
    Vec<NativeDirectoryTreeLine>,
    usize,
    usize,
    usize,
    usize,
    bool,
);
pub(super) type NativeFileChange = (
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<NativeObservationReceipt>,
    u64,
    u64,
);
pub(super) type NativePostEditSource = (
    String,
    NativeObservationReceipt,
    Vec<NativeRenderedLine>,
    bool,
);
pub(super) type NativeEditResult = (
    String,
    u64,
    u64,
    Vec<NativeFileChange>,
    Vec<NativePostEditSource>,
    bool,
    bool,
    Option<String>,
    Option<f64>,
);
pub(super) type NativeObservedSource = (NativeObservationReceipt, Vec<NativeRenderedLine>);

impl From<ObservationReceipt> for NativeObservationReceipt {
    fn from(receipt: ObservationReceipt) -> Self {
        (
            receipt.path,
            receipt.tag,
            receipt.content_sha256,
            receipt.generation,
            native_items(receipt.visible_ranges),
            receipt.complete_presentation,
        )
    }
}

impl From<RenderedLine> for NativeRenderedLine {
    fn from(line: RenderedLine) -> Self {
        (line.number, line.short_hash, line.text)
    }
}

impl From<WorkspaceTextSerialization> for NativeTextSerialization {
    fn from(value: WorkspaceTextSerialization) -> Self {
        let line_ending = match value.line_ending {
            LineEnding::Lf => "lf",
            LineEnding::CrLf => "crlf",
            LineEnding::Cr => "cr",
        };
        (value.bom, line_ending.to_owned(), value.terminal_newline)
    }
}

impl From<WorkspaceFileRead> for NativeFileRead {
    fn from(result: WorkspaceFileRead) -> Self {
        (
            result.path,
            result.observation.map(Into::into),
            native_items(result.lines),
            result.total_lines,
            result.complete_presentation,
            result.editable,
            result.total_bytes,
            result.observation_limit,
            result.serialization.map(Into::into),
        )
    }
}

impl From<WorkspaceDirectoryRead> for NativeDirectoryRead {
    fn from(result: WorkspaceDirectoryRead) -> Self {
        (result.path, native_items(result.entries), result.truncated)
    }
}

impl From<WorkspaceDirectoryTreeLine> for NativeDirectoryTreeLine {
    fn from(line: WorkspaceDirectoryTreeLine) -> Self {
        (line.depth, line.name, line.kind, line.omitted_entries)
    }
}

impl From<WorkspaceDirectoryTreeRead> for NativeDirectoryTreeRead {
    fn from(result: WorkspaceDirectoryTreeRead) -> Self {
        (
            result.path,
            native_items(result.lines),
            result.child_limit,
            result.scanned_entries,
            result.limited_directories,
            result.omitted_entries,
            result.scan_truncated,
        )
    }
}

impl From<FileChange> for NativeFileChange {
    fn from(change: FileChange) -> Self {
        (
            change.path,
            change.operation,
            change.destination,
            change.before_sha256,
            change.after_sha256,
            change.observation.map(Into::into),
            change.file_generation,
            change.revision,
        )
    }
}

impl From<PostEditSource> for NativePostEditSource {
    fn from(source: PostEditSource) -> Self {
        (
            source.path,
            source.observation.into(),
            native_items(source.lines),
            source.complete_presentation,
        )
    }
}

impl From<EditResult> for NativeEditResult {
    fn from(result: EditResult) -> Self {
        (
            result.mode,
            result.mode_generation,
            result.policy_generation,
            native_items(result.changes),
            native_items(result.post_edit_sources),
            result.preflight_complete,
            result.commit_complete,
            result.matching_strategy,
            result.confidence,
        )
    }
}

impl From<LineRange> for (usize, usize) {
    fn from(range: LineRange) -> Self {
        (range.start, range.end)
    }
}

impl From<WorkspaceDirectoryEntry> for (String, String, Option<u64>) {
    fn from(entry: WorkspaceDirectoryEntry) -> Self {
        (entry.path, entry.kind, entry.size)
    }
}

pub(super) fn native_items<T, U>(values: Vec<T>) -> Vec<U>
where
    U: From<T>,
{
    values.into_iter().map(U::from).collect()
}
