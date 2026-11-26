use std::path::Path;

use ast_grep_core::{AstGrep, MatchStrictness, tree_sitter::StrDoc};
use ast_grep_language::{LanguageExt, SupportLang};

use crate::ast::{
    AstError,
    language::{canonical_name, infer_language, resolve_language, strictness},
    types::{Issue, ScanOptions, SourceScope},
};
use crate::workspace::{
    MetadataLevel, ReadExtent, ScanFileKind, ScanOrder, ScanRequest, WorkCompletion, WorkControl,
    Workspace, WorkspaceEntry, read_content,
};

pub(crate) struct Selection<'a> {
    pub files: Vec<(WorkspaceEntry, SupportLang)>,
    pub unsupported_files: usize,
    pub strictness: MatchStrictness,
    pub reader: TreeReader<'a>,
}

pub(crate) struct TreeReader<'a> {
    max_bytes: usize,
    cancellation: &'a crate::workspace::Cancellation,
    pub issues: Vec<Issue>,
    pub files_searched: usize,
}

pub(crate) fn select<'a>(root: &Path, scope: &'a SourceScope) -> Result<Selection<'a>, AstError> {
    let explicit = scope
        .language
        .as_deref()
        .map(resolve_language)
        .transpose()?;
    let strictness = strictness(&scope.strictness)?;
    let selected = discover_files(
        root,
        &scope.scan,
        scope.limits.max_files,
        &scope.cancellation,
    )?;
    let (files, unsupported_files) = classify_files(selected, explicit);
    Ok(Selection {
        files,
        unsupported_files,
        strictness,
        reader: TreeReader {
            max_bytes: scope.limits.max_file_bytes,
            cancellation: &scope.cancellation,
            issues: Vec::new(),
            files_searched: 0,
        },
    })
}

fn discover_files(
    root: &Path,
    options: &ScanOptions,
    max_files: usize,
    cancellation: &crate::workspace::Cancellation,
) -> Result<Vec<WorkspaceEntry>, AstError> {
    let workspace = Workspace::from_canonical(root);
    let control = WorkControl::new(cancellation.clone(), None);
    let result = workspace.scan(
        &ScanRequest {
            selections: options.paths.clone(),
            include_hidden: options.include_hidden,
            respect_gitignore: options.respect_gitignore,
            include_node_modules: options.include_node_modules,
            file_kind: ScanFileKind::Files,
            metadata: MetadataLevel::Size,
            order: ScanOrder::Path,
            max_files,
        },
        &control,
    )?;
    if result.completion != WorkCompletion::Complete {
        return Err(AstError::Limit(format!(
            "workspace selection exceeds the {max_files} file limit"
        )));
    }

    Ok(result.entries)
}

fn classify_files(
    selected: Vec<WorkspaceEntry>,
    explicit: Option<SupportLang>,
) -> (Vec<(WorkspaceEntry, SupportLang)>, usize) {
    let mut files = Vec::new();
    let mut unsupported = 0;
    for candidate in selected {
        match explicit.or_else(|| infer_language(&candidate.path)) {
            Some(language) => files.push((candidate, language)),
            None => unsupported += 1,
        }
    }
    (files, unsupported)
}

impl TreeReader<'_> {
    pub(crate) fn read(
        &mut self,
        candidate: &WorkspaceEntry,
        language: SupportLang,
    ) -> Result<Option<AstGrep<StrDoc<SupportLang>>>, AstError> {
        if self.cancellation.is_cancelled() {
            return Err(AstError::Cancelled);
        }
        let source = match read_source(candidate, self.max_bytes, self.cancellation) {
            Ok(source) => source,
            Err(issue) => {
                if self.cancellation.is_cancelled() {
                    return Err(AstError::Cancelled);
                }
                self.issues.push(issue);
                return Ok(None);
            }
        };
        let tree = language.ast_grep(source);
        if tree
            .root()
            .dfs()
            .any(|node| node.is_error() || node.is_missing())
        {
            self.issues.push(parse_issue(candidate, language));
            return Ok(None);
        }
        self.files_searched += 1;
        Ok(Some(tree))
    }
}

pub(crate) fn read_source(
    candidate: &WorkspaceEntry,
    max_bytes: usize,
    cancellation: &crate::workspace::Cancellation,
) -> Result<String, Issue> {
    let control = WorkControl::new(cancellation.clone(), None);
    let content = read_content(
        &candidate.path,
        ReadExtent::Complete {
            max_bytes: max_bytes as u64,
        },
        &control,
    )
    .map_err(|error| issue(candidate, "read_error", workspace_read_message(error)))?;
    if !content.complete {
        return Err(issue(
            candidate,
            "limit_reached",
            format!("file exceeds the {max_bytes} byte limit"),
        ));
    }
    if content.binary {
        return Err(issue(
            candidate,
            "read_error",
            "file contains a NUL byte".to_owned(),
        ));
    }
    String::from_utf8(content.bytes).map_err(|_| {
        issue(
            candidate,
            "read_error",
            "file is not valid UTF-8".to_owned(),
        )
    })
}

pub(crate) fn parse_issue(candidate: &WorkspaceEntry, language: SupportLang) -> Issue {
    (
        Some(candidate.relative.clone()),
        Some(canonical_name(language).to_owned()),
        "parse_error".to_owned(),
        "source contains syntax-tree error nodes".to_owned(),
    )
}

fn issue(candidate: &WorkspaceEntry, kind: &str, message: String) -> Issue {
    (
        Some(candidate.relative.clone()),
        None,
        kind.to_owned(),
        message,
    )
}

fn workspace_read_message(error: crate::workspace::WorkspaceError) -> String {
    match error {
        crate::workspace::WorkspaceError::Read(message) => message,
        crate::workspace::WorkspaceError::Cancelled => "AST operation cancelled".to_owned(),
        crate::workspace::WorkspaceError::Deadline => {
            "AST operation reached its deadline".to_owned()
        }
        other => format!("{other:?}"),
    }
}
