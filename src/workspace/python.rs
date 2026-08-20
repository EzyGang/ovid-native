use std::time::Duration;

use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyValueError};
use pyo3::prelude::*;

use crate::workspace::{
    Cancellation, DiscoveryRequest, DiscoveryResult, EditResult, FileChange, HashlineOperation,
    HashlineSection, LineEnding, LineRange, MutationContext, ObservationReceipt, PolicyGeneration,
    PostEditSource, RenderedLine, WorkCompletion, WorkControl, Workspace, WorkspaceDirectoryRead,
    WorkspaceError, WorkspaceFileRead, WorkspacePolicy, WorkspaceTextSerialization,
    parse_apply_patch, parse_structured_patch,
};

create_exception!(_native, NativeWorkspaceReadError, PyException);
create_exception!(
    _native,
    NativeWorkspaceEncodingError,
    NativeWorkspaceReadError
);
create_exception!(
    _native,
    NativeWorkspaceBinaryFileError,
    NativeWorkspaceReadError
);
create_exception!(_native, NativeWorkspaceLimitError, PyException);
create_exception!(
    _native,
    NativeWorkspaceObservationNotFoundError,
    PyException
);
create_exception!(
    _native,
    NativeWorkspaceObservationCollisionError,
    PyException
);
create_exception!(_native, NativeWorkspaceUnseenLineError, PyException);
create_exception!(
    _native,
    NativeWorkspaceObservedLineChangedError,
    PyException
);
create_exception!(_native, NativeWorkspaceStaleError, PyException);
create_exception!(_native, NativeWorkspaceEditModeError, PyException);
create_exception!(_native, NativeWorkspacePatchError, PyException);
create_exception!(_native, NativeWorkspacePartialCommitError, PyException);
create_exception!(_native, NativeWorkspaceWriteError, PyException);
create_exception!(_native, NativeWorkspacePathError, PyException);
create_exception!(_native, NativeWorkspaceClosedError, PyException);

#[pyclass(frozen, skip_from_py_object)]
#[derive(Clone, Debug)]
pub(crate) struct NativeWorkspace {
    pub(crate) inner: Workspace,
}

impl NativeWorkspace {
    fn new(workspace: Workspace) -> Self {
        Self { inner: workspace }
    }
}

#[pymethods]
impl NativeWorkspace {
    #[getter]
    fn root(&self) -> String {
        self.inner.root().to_string_lossy().into_owned()
    }
}

#[pyclass(frozen, skip_from_py_object)]
#[derive(Clone, Debug)]
struct NativeWorkspaceMutation {
    context: MutationContext,
}

#[pymethods]
impl NativeWorkspaceMutation {
    #[getter]
    fn mode(&self) -> &str {
        &self.context.mode
    }

    #[getter]
    fn mode_generation(&self) -> u64 {
        self.context.mode_generation
    }

    #[getter]
    fn policy_generation(&self) -> u64 {
        self.context.policy_generation
    }
}

#[pyclass(frozen, skip_from_py_object)]
#[derive(Clone, Debug)]
struct NativeWorkspaceCancellation {
    inner: Cancellation,
}

#[pymethods]
impl NativeWorkspaceCancellation {
    #[new]
    fn new() -> Self {
        Self {
            inner: Cancellation::new(),
        }
    }

    fn cancel(&self) {
        self.inner.cancel();
    }
}

impl NativeWorkspaceCancellation {
    fn token(&self) -> Cancellation {
        self.inner.clone()
    }
}

type NativeObservationReceipt = (String, String, String, u64, Vec<(usize, usize)>, bool);
type NativeRenderedLine = (usize, String, String);
type NativeTextSerialization = (bool, String, bool);
type NativeFileRead = (
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
type NativeDirectoryRead = (String, Vec<(String, String, Option<u64>)>, bool);
type NativeDiscoveryResult = (Vec<String>, String);
type NativeFileChange = (
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<NativeObservationReceipt>,
    u64,
    u64,
);
type NativePostEditSource = (
    String,
    NativeObservationReceipt,
    Vec<NativeRenderedLine>,
    bool,
);
type NativeEditResult = (
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
type NativePolicy = (bool, f64, u64, u64, usize, usize, bool, u64);
type NativeHashlineOperation = (
    String,
    Option<usize>,
    Option<String>,
    Option<usize>,
    Option<String>,
    Vec<String>,
    Option<String>,
    Option<String>,
);
type NativeHashlineSection = (String, String, Vec<NativeHashlineOperation>);
type NativeObservedSource = (NativeObservationReceipt, Vec<NativeRenderedLine>);

#[pyfunction]
fn workspace_create(root: String) -> PyResult<NativeWorkspace> {
    Workspace::new(&root)
        .map(NativeWorkspace::new)
        .map_err(to_python_error)
}

#[pyfunction]
fn workspace_close(workspace: PyRef<'_, NativeWorkspace>) {
    workspace.inner.close();
}

#[pyfunction]
fn workspace_is_closed(workspace: PyRef<'_, NativeWorkspace>) -> bool {
    workspace.inner.is_closed()
}

#[pyfunction]
fn workspace_revision(workspace: PyRef<'_, NativeWorkspace>) -> u64 {
    workspace.inner.revision()
}

#[pyfunction]
fn workspace_policy(workspace: PyRef<'_, NativeWorkspace>) -> PyResult<NativePolicy> {
    workspace
        .inner
        .policy()
        .map(policy_to_native)
        .map_err(to_python_error)
}

#[pyfunction]
fn workspace_set_policy(
    workspace: PyRef<'_, NativeWorkspace>,
    policy: (bool, f64, u64, u64, usize, usize, bool),
) -> PyResult<NativePolicy> {
    let (
        allow_fuzzy_replace,
        fuzzy_replace_threshold,
        max_read_bytes,
        max_observation_file_bytes,
        max_observation_entries,
        max_observation_store_bytes,
        create_parent_directories,
    ) = policy;
    workspace
        .inner
        .set_policy(WorkspacePolicy {
            allow_fuzzy_replace,
            fuzzy_replace_threshold,
            max_read_bytes,
            max_observation_file_bytes,
            max_observation_entries,
            max_observation_store_bytes,
            create_parent_directories,
        })
        .map(policy_to_native)
        .map_err(to_python_error)
}

#[pyfunction]
fn workspace_edit_mode(workspace: PyRef<'_, NativeWorkspace>) -> PyResult<(String, u64)> {
    workspace
        .inner
        .edit_mode()
        .map(|selection| (selection.mode, selection.generation))
        .map_err(to_python_error)
}

#[pyfunction]
fn workspace_set_edit_mode(
    workspace: PyRef<'_, NativeWorkspace>,
    mode: String,
) -> PyResult<(String, u64)> {
    workspace
        .inner
        .set_edit_mode(&mode)
        .map(|selection| (selection.mode, selection.generation))
        .map_err(to_python_error)
}

#[pyfunction]
#[pyo3(signature = (workspace, mode=None))]
fn workspace_capture_mutation(
    workspace: PyRef<'_, NativeWorkspace>,
    mode: Option<String>,
) -> PyResult<NativeWorkspaceMutation> {
    let selection = workspace.inner.edit_mode().map_err(to_python_error)?;
    let policy = workspace.inner.policy().map_err(to_python_error)?;
    let captured_mode = mode.unwrap_or(selection.mode);
    if !workspace
        .inner
        .supports_edit_mode(&captured_mode)
        .map_err(to_python_error)?
    {
        return Err(to_python_error(WorkspaceError::EditMode(format!(
            "workspace edit mode is not registered: {captured_mode}"
        ))));
    }

    Ok(NativeWorkspaceMutation {
        context: MutationContext {
            mode: captured_mode,
            mode_generation: selection.generation,
            policy_generation: policy.generation,
            cancellation: Cancellation::new(),
            policy: policy.policy,
        },
    })
}

#[pyfunction]
fn workspace_register_edit_mode(
    workspace: PyRef<'_, NativeWorkspace>,
    mode: String,
) -> PyResult<()> {
    workspace
        .inner
        .register_edit_mode(&mode)
        .map_err(to_python_error)
}

#[pyfunction]
fn workspace_discover_files(
    py: Python<'_>,
    workspace: PyRef<'_, NativeWorkspace>,
    filename: String,
    max_depth: usize,
    limit: usize,
    timeout_seconds: f64,
    cancellation: PyRef<'_, NativeWorkspaceCancellation>,
) -> PyResult<NativeDiscoveryResult> {
    if !timeout_seconds.is_finite() || timeout_seconds <= 0.0 || timeout_seconds > 30.0 {
        return Err(PyValueError::new_err(
            "workspace discovery timeout must be greater than zero and at most 30 seconds",
        ));
    }
    let workspace = workspace.inner.clone();
    let control = WorkControl::new(
        cancellation.token(),
        Some(Duration::from_secs_f64(timeout_seconds)),
    );
    py.detach(move || {
        workspace
            .discover_files(
                &DiscoveryRequest {
                    filename,
                    max_depth,
                    limit,
                },
                &control,
            )
            .map(discovery_to_native)
            .map_err(discovery_to_python_error)
    })
}

#[pyfunction]
fn workspace_read_file(
    py: Python<'_>,
    workspace: PyRef<'_, NativeWorkspace>,
    path: String,
    ranges: Vec<(usize, Option<usize>)>,
) -> PyResult<NativeFileRead> {
    let workspace = workspace.inner.clone();
    py.detach(move || {
        let ranges = ranges
            .into_iter()
            .map(|(start, end)| LineRange {
                start,
                end: end.unwrap_or(usize::MAX),
            })
            .collect::<Vec<_>>();
        workspace
            .read_file(&path, &ranges)
            .map(file_read_to_native)
            .map_err(to_python_error)
    })
}

#[pyfunction]
fn workspace_list_directory(
    py: Python<'_>,
    workspace: PyRef<'_, NativeWorkspace>,
    path: String,
    depth: usize,
) -> PyResult<NativeDirectoryRead> {
    let workspace = workspace.inner.clone();
    py.detach(move || {
        workspace
            .list_directory(&path, depth)
            .map(directory_read_to_native)
            .map_err(to_python_error)
    })
}

#[pyfunction]
fn workspace_resolve_observation(
    py: Python<'_>,
    workspace: PyRef<'_, NativeWorkspace>,
    path: String,
    tag: String,
) -> PyResult<NativeObservationReceipt> {
    let workspace = workspace.inner.clone();
    py.detach(move || {
        workspace
            .resolve_observation(&path, &tag)
            .map(receipt_to_native)
            .map_err(to_python_error)
    })
}

#[pyfunction]
fn workspace_validate_observed_lines(
    py: Python<'_>,
    workspace: PyRef<'_, NativeWorkspace>,
    path: String,
    tag: String,
    lines: Vec<usize>,
) -> PyResult<NativeObservationReceipt> {
    let workspace = workspace.inner.clone();
    py.detach(move || {
        workspace
            .validate_observed_lines(&path, &tag, &lines)
            .map(receipt_to_native)
            .map_err(to_python_error)
    })
}

#[pyfunction]
fn workspace_observe_source_lines(
    py: Python<'_>,
    workspace: PyRef<'_, NativeWorkspace>,
    path: String,
    claims: Vec<(usize, String)>,
    spans: Vec<(usize, usize, usize, usize)>,
    complete_presentation: bool,
) -> PyResult<NativeObservedSource> {
    let workspace = workspace.inner.clone();
    py.detach(move || {
        workspace
            .observe_source_lines(&path, &claims, &spans, complete_presentation)
            .map(|(receipt, lines)| {
                (
                    receipt_to_native(receipt),
                    lines.into_iter().map(line_to_native).collect(),
                )
            })
            .map_err(to_python_error)
    })
}

#[pyfunction]
fn workspace_create_file(
    py: Python<'_>,
    workspace: PyRef<'_, NativeWorkspace>,
    path: String,
    content: String,
    create_parents: bool,
    cancellation: PyRef<'_, NativeWorkspaceCancellation>,
) -> PyResult<NativeEditResult> {
    let workspace = workspace.inner.clone();
    let cancellation = cancellation.token();
    py.detach(move || {
        let policy = workspace.policy().map_err(to_python_error)?;
        workspace
            .create_text_file(
                &path,
                &content,
                create_parents,
                &MutationContext::write(policy, cancellation),
            )
            .map(edit_result_to_native)
            .map_err(to_python_error)
    })
}

#[pyfunction]
fn workspace_replace_file(
    py: Python<'_>,
    workspace: PyRef<'_, NativeWorkspace>,
    path: String,
    content: String,
    expected_observation: String,
    cancellation: PyRef<'_, NativeWorkspaceCancellation>,
) -> PyResult<NativeEditResult> {
    let workspace = workspace.inner.clone();
    let cancellation = cancellation.token();
    py.detach(move || {
        let policy = workspace.policy().map_err(to_python_error)?;
        workspace
            .replace_text_file(
                &path,
                &content,
                &expected_observation,
                &MutationContext::write(policy, cancellation),
            )
            .map(edit_result_to_native)
            .map_err(to_python_error)
    })
}

#[pyfunction]
fn workspace_replace_text(
    py: Python<'_>,
    workspace: PyRef<'_, NativeWorkspace>,
    mutation: PyRef<'_, NativeWorkspaceMutation>,
    path: String,
    replacement: (String, String, bool),
    cancellation: PyRef<'_, NativeWorkspaceCancellation>,
) -> PyResult<NativeEditResult> {
    let workspace = workspace.inner.clone();
    let mut context = mutation.context.clone();
    context.cancellation = cancellation.token();
    py.detach(move || {
        workspace
            .replace_text(
                &path,
                &replacement.0,
                &replacement.1,
                replacement.2,
                &context,
            )
            .map(edit_result_to_native)
            .map_err(to_python_error)
    })
}

#[pyfunction]
fn workspace_patch(
    py: Python<'_>,
    workspace: PyRef<'_, NativeWorkspace>,
    mutation: PyRef<'_, NativeWorkspaceMutation>,
    path: String,
    edits: Vec<(String, Option<String>, Option<String>)>,
    cancellation: PyRef<'_, NativeWorkspaceCancellation>,
) -> PyResult<NativeEditResult> {
    let workspace = workspace.inner.clone();
    let mut context = mutation.context.clone();
    context.cancellation = cancellation.token();
    py.detach(move || {
        let operations = parse_structured_patch(&path, &edits).map_err(to_python_error)?;
        workspace
            .apply_patch_operations(&operations, &context, "patch")
            .map(edit_result_to_native)
            .map_err(to_python_error)
    })
}

#[pyfunction]
fn workspace_apply_patch(
    py: Python<'_>,
    workspace: PyRef<'_, NativeWorkspace>,
    mutation: PyRef<'_, NativeWorkspaceMutation>,
    input: String,
    cancellation: PyRef<'_, NativeWorkspaceCancellation>,
) -> PyResult<NativeEditResult> {
    let workspace = workspace.inner.clone();
    let mut context = mutation.context.clone();
    context.cancellation = cancellation.token();
    py.detach(move || {
        let operations = parse_apply_patch(&input).map_err(to_python_error)?;
        workspace
            .apply_patch_operations(&operations, &context, "apply_patch")
            .map(edit_result_to_native)
            .map_err(to_python_error)
    })
}

#[pyfunction]
fn workspace_hashline(
    py: Python<'_>,
    workspace: PyRef<'_, NativeWorkspace>,
    mutation: PyRef<'_, NativeWorkspaceMutation>,
    sections: Vec<NativeHashlineSection>,
    cancellation: PyRef<'_, NativeWorkspaceCancellation>,
) -> PyResult<NativeEditResult> {
    let workspace = workspace.inner.clone();
    let mut context = mutation.context.clone();
    context.cancellation = cancellation.token();
    let sections = sections
        .into_iter()
        .map(|(path, tag, operations)| HashlineSection {
            path,
            tag,
            operations: operations
                .into_iter()
                .map(
                    |(kind, start, start_hash, end, end_hash, body, register, destination)| {
                        HashlineOperation {
                            kind,
                            start,
                            start_hash,
                            end,
                            end_hash,
                            body,
                            register,
                            destination,
                        }
                    },
                )
                .collect(),
        })
        .collect::<Vec<_>>();
    py.detach(move || {
        workspace
            .apply_hashline(&sections, &context)
            .map(edit_result_to_native)
            .map_err(to_python_error)
    })
}

#[pyfunction]
fn workspace_delete_file(
    py: Python<'_>,
    workspace: PyRef<'_, NativeWorkspace>,
    path: String,
    cancellation: PyRef<'_, NativeWorkspaceCancellation>,
) -> PyResult<NativeEditResult> {
    let workspace = workspace.inner.clone();
    let cancellation = cancellation.token();
    py.detach(move || {
        let policy = workspace.policy().map_err(to_python_error)?;
        workspace
            .delete_text_file(&path, &MutationContext::write(policy, cancellation))
            .map(edit_result_to_native)
            .map_err(to_python_error)
    })
}

#[pyfunction]
fn workspace_move_file(
    py: Python<'_>,
    workspace: PyRef<'_, NativeWorkspace>,
    path: String,
    destination: String,
    cancellation: PyRef<'_, NativeWorkspaceCancellation>,
) -> PyResult<NativeEditResult> {
    let workspace = workspace.inner.clone();
    let cancellation = cancellation.token();
    py.detach(move || {
        let policy = workspace.policy().map_err(to_python_error)?;
        workspace
            .move_text_file(
                &path,
                &destination,
                &MutationContext::write(policy, cancellation),
            )
            .map(edit_result_to_native)
            .map_err(to_python_error)
    })
}
pub(crate) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<NativeWorkspace>()?;
    module.add_class::<NativeWorkspaceMutation>()?;
    module.add_class::<NativeWorkspaceCancellation>()?;
    module.add(
        "NativeWorkspaceReadError",
        module.py().get_type::<NativeWorkspaceReadError>(),
    )?;
    module.add(
        "NativeWorkspaceEncodingError",
        module.py().get_type::<NativeWorkspaceEncodingError>(),
    )?;
    module.add(
        "NativeWorkspaceBinaryFileError",
        module.py().get_type::<NativeWorkspaceBinaryFileError>(),
    )?;
    module.add(
        "NativeWorkspaceLimitError",
        module.py().get_type::<NativeWorkspaceLimitError>(),
    )?;
    module.add(
        "NativeWorkspaceObservationNotFoundError",
        module
            .py()
            .get_type::<NativeWorkspaceObservationNotFoundError>(),
    )?;
    module.add(
        "NativeWorkspaceObservationCollisionError",
        module
            .py()
            .get_type::<NativeWorkspaceObservationCollisionError>(),
    )?;
    module.add(
        "NativeWorkspaceUnseenLineError",
        module.py().get_type::<NativeWorkspaceUnseenLineError>(),
    )?;
    module.add(
        "NativeWorkspaceObservedLineChangedError",
        module
            .py()
            .get_type::<NativeWorkspaceObservedLineChangedError>(),
    )?;
    module.add(
        "NativeWorkspaceStaleError",
        module.py().get_type::<NativeWorkspaceStaleError>(),
    )?;
    module.add(
        "NativeWorkspaceEditModeError",
        module.py().get_type::<NativeWorkspaceEditModeError>(),
    )?;
    module.add(
        "NativeWorkspacePatchError",
        module.py().get_type::<NativeWorkspacePatchError>(),
    )?;
    module.add(
        "NativeWorkspacePartialCommitError",
        module.py().get_type::<NativeWorkspacePartialCommitError>(),
    )?;
    module.add(
        "NativeWorkspaceWriteError",
        module.py().get_type::<NativeWorkspaceWriteError>(),
    )?;
    module.add(
        "NativeWorkspacePathError",
        module.py().get_type::<NativeWorkspacePathError>(),
    )?;
    module.add(
        "NativeWorkspaceClosedError",
        module.py().get_type::<NativeWorkspaceClosedError>(),
    )?;
    module.add_function(wrap_pyfunction!(workspace_create, module)?)?;
    module.add_function(wrap_pyfunction!(workspace_close, module)?)?;
    module.add_function(wrap_pyfunction!(workspace_is_closed, module)?)?;
    module.add_function(wrap_pyfunction!(workspace_revision, module)?)?;
    module.add_function(wrap_pyfunction!(workspace_policy, module)?)?;
    module.add_function(wrap_pyfunction!(workspace_set_policy, module)?)?;
    module.add_function(wrap_pyfunction!(workspace_edit_mode, module)?)?;
    module.add_function(wrap_pyfunction!(workspace_set_edit_mode, module)?)?;
    module.add_function(wrap_pyfunction!(workspace_capture_mutation, module)?)?;
    module.add_function(wrap_pyfunction!(workspace_register_edit_mode, module)?)?;
    module.add_function(wrap_pyfunction!(workspace_discover_files, module)?)?;
    module.add_function(wrap_pyfunction!(workspace_read_file, module)?)?;
    module.add_function(wrap_pyfunction!(workspace_list_directory, module)?)?;
    module.add_function(wrap_pyfunction!(workspace_resolve_observation, module)?)?;
    module.add_function(wrap_pyfunction!(workspace_validate_observed_lines, module)?)?;
    module.add_function(wrap_pyfunction!(workspace_observe_source_lines, module)?)?;
    module.add_function(wrap_pyfunction!(workspace_create_file, module)?)?;
    module.add_function(wrap_pyfunction!(workspace_replace_file, module)?)?;
    module.add_function(wrap_pyfunction!(workspace_replace_text, module)?)?;
    module.add_function(wrap_pyfunction!(workspace_patch, module)?)?;
    module.add_function(wrap_pyfunction!(workspace_apply_patch, module)?)?;
    module.add_function(wrap_pyfunction!(workspace_hashline, module)?)?;
    module.add_function(wrap_pyfunction!(workspace_delete_file, module)?)?;
    module.add_function(wrap_pyfunction!(workspace_move_file, module)?)?;
    Ok(())
}

fn discovery_to_native(result: DiscoveryResult) -> NativeDiscoveryResult {
    let completion = match result.completion {
        WorkCompletion::Complete => "complete",
        WorkCompletion::FileLimitReached => "file_limit_reached",
        WorkCompletion::DeadlineReached => "deadline_reached",
    };
    (result.paths, completion.to_owned())
}

fn discovery_to_python_error(error: WorkspaceError) -> PyErr {
    match error {
        WorkspaceError::Cancelled => {
            NativeWorkspaceReadError::new_err("workspace discovery was cancelled")
        }
        other => to_python_error(other),
    }
}

fn policy_to_native(generation: PolicyGeneration) -> NativePolicy {
    let policy = generation.policy;
    (
        policy.allow_fuzzy_replace,
        policy.fuzzy_replace_threshold,
        policy.max_read_bytes,
        policy.max_observation_file_bytes,
        policy.max_observation_entries,
        policy.max_observation_store_bytes,
        policy.create_parent_directories,
        generation.generation,
    )
}

fn receipt_to_native(receipt: ObservationReceipt) -> NativeObservationReceipt {
    (
        receipt.path,
        receipt.tag,
        receipt.content_sha256,
        receipt.generation,
        receipt
            .visible_ranges
            .into_iter()
            .map(|range| (range.start, range.end))
            .collect(),
        receipt.complete_presentation,
    )
}

fn line_to_native(line: RenderedLine) -> NativeRenderedLine {
    (line.number, line.short_hash, line.text)
}

fn serialization_to_native(value: WorkspaceTextSerialization) -> NativeTextSerialization {
    let line_ending = match value.line_ending {
        LineEnding::Lf => "lf",
        LineEnding::CrLf => "crlf",
        LineEnding::Cr => "cr",
    };
    (value.bom, line_ending.to_owned(), value.terminal_newline)
}
fn file_read_to_native(result: WorkspaceFileRead) -> NativeFileRead {
    (
        result.path,
        result.observation.map(receipt_to_native),
        result.lines.into_iter().map(line_to_native).collect(),
        result.total_lines,
        result.complete_presentation,
        result.editable,
        result.total_bytes,
        result.observation_limit,
        result.serialization.map(serialization_to_native),
    )
}

fn directory_read_to_native(result: WorkspaceDirectoryRead) -> NativeDirectoryRead {
    (
        result.path,
        result
            .entries
            .into_iter()
            .map(|entry| (entry.path, entry.kind, entry.size))
            .collect(),
        result.truncated,
    )
}

fn change_to_native(change: FileChange) -> NativeFileChange {
    (
        change.path,
        change.operation,
        change.destination,
        change.before_sha256,
        change.after_sha256,
        change.observation.map(receipt_to_native),
        change.file_generation,
        change.revision,
    )
}

fn post_edit_to_native(source: PostEditSource) -> NativePostEditSource {
    (
        source.path,
        receipt_to_native(source.observation),
        source.lines.into_iter().map(line_to_native).collect(),
        source.complete_presentation,
    )
}

fn edit_result_to_native(result: EditResult) -> NativeEditResult {
    (
        result.mode,
        result.mode_generation,
        result.policy_generation,
        result.changes.into_iter().map(change_to_native).collect(),
        result
            .post_edit_sources
            .into_iter()
            .map(post_edit_to_native)
            .collect(),
        result.preflight_complete,
        result.commit_complete,
        result.matching_strategy,
        result.confidence,
    )
}

fn to_python_error(error: WorkspaceError) -> PyErr {
    match error {
        WorkspaceError::Configuration(message) => PyValueError::new_err(message),
        WorkspaceError::Path(message) => NativeWorkspacePathError::new_err(message),
        WorkspaceError::Read(message) => NativeWorkspaceReadError::new_err(message),
        WorkspaceError::Encoding(message) => NativeWorkspaceEncodingError::new_err(message),
        WorkspaceError::Binary(message) => NativeWorkspaceBinaryFileError::new_err(message),
        WorkspaceError::Limit(message) => NativeWorkspaceLimitError::new_err(message),
        WorkspaceError::ObservationNotFound(message) => {
            NativeWorkspaceObservationNotFoundError::new_err(message)
        }
        WorkspaceError::ObservationCollision(message) => {
            NativeWorkspaceObservationCollisionError::new_err(message)
        }
        WorkspaceError::UnseenLine(message) => NativeWorkspaceUnseenLineError::new_err(message),
        WorkspaceError::ObservedLineChanged(message) => {
            NativeWorkspaceObservedLineChangedError::new_err(message)
        }
        WorkspaceError::Stale(message) => NativeWorkspaceStaleError::new_err(message),
        WorkspaceError::EditMode(message) => NativeWorkspaceEditModeError::new_err(message),
        WorkspaceError::Patch(message) => NativeWorkspacePatchError::new_err(message),
        WorkspaceError::PartialCommit {
            landed,
            pending,
            changes,
        } => NativeWorkspacePartialCommitError::new_err((
            "workspace patch committed only part of its operations",
            landed,
            pending,
            changes
                .into_iter()
                .map(change_to_native)
                .collect::<Vec<_>>(),
        )),
        WorkspaceError::Write(message) => NativeWorkspaceWriteError::new_err(message),
        WorkspaceError::Cancelled => {
            NativeWorkspaceWriteError::new_err("workspace operation was cancelled")
        }
        WorkspaceError::Closed => NativeWorkspaceClosedError::new_err("workspace is closed"),
        WorkspaceError::Deadline => {
            NativeWorkspaceReadError::new_err("workspace operation reached its deadline")
        }
    }
}
