use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use crate::ast::AstError;
use crate::ast::rewrite::{apply, preview};
use crate::ast::search::search as workspace_search;
use crate::ast::types::{
    Cancellation, Limits, NativeAstCancellation, RewriteRequest, ScanOptions, SearchRequest,
};
use crate::workspace::Workspace;

fn limits() -> Limits {
    Limits {
        max_matches: 100,
        max_files: 100,
        max_file_bytes: 100_000,
        max_replacements: 100,
        max_changed_files: 100,
    }
}

fn cancellation() -> Cancellation {
    NativeAstCancellation::new().token()
}

fn scan(paths: &[&str]) -> ScanOptions {
    ScanOptions {
        paths: paths.iter().map(|path| (*path).to_owned()).collect(),
        include_hidden: false,
        respect_gitignore: true,
        include_node_modules: false,
    }
}

fn search_request(pattern: &str, paths: &[&str]) -> SearchRequest {
    SearchRequest {
        pattern: pattern.to_owned(),
        scan: scan(paths),
        language: None,
        strictness: "smart".to_owned(),
        offset: 0,
        limit: 50,
        include_captures: true,
        limits: limits(),
        cancellation: cancellation(),
    }
}

fn workspace() -> TempDir {
    tempfile::tempdir().expect("temporary workspace")
}

fn canonical(root: &TempDir) -> PathBuf {
    root.path().canonicalize().expect("canonical workspace")
}

fn search(
    root: &Path,
    request: SearchRequest,
) -> Result<crate::ast::types::SearchResult, AstError> {
    let workspace = Workspace::from_canonical(root);
    workspace_search(&workspace, request)
}

#[test]
fn search_matches_structure_captures_and_utf8_ranges() {
    let root = workspace();
    fs::write(
        root.path().join("sample.py"),
        "é = 1\nprint(é)\n# print(ignored)\ntext = 'print(ignored)'\n",
    )
    .expect("source file");

    let result = search(&canonical(&root), search_request("print($A)", &["."])).expect("search");

    assert_eq!(result.1, 1);
    assert_eq!(result.0[0].2, "print(é)");
    assert_eq!(result.0[0].3, ((2, 1, 7), (2, 9, 16)));
    assert_eq!(result.0[0].4[0].0, "A");
    assert_eq!(result.0[0].4[0].1, "é");
}

#[test]
fn search_supports_variadic_repeated_and_pagination() {
    let root = workspace();
    fs::write(
        root.path().join("sample.js"),
        "same(a, a);\nsame(a, b);\ncall(1, 2, 3);\nsame(c, c);\n",
    )
    .expect("source file");
    let repeated = search(
        &canonical(&root),
        search_request("same($A, $A)", &["sample.js"]),
    )
    .expect("search");
    assert_eq!(repeated.1, 2);

    let variadic = search(
        &canonical(&root),
        search_request("call($$$ARGS)", &["sample.js"]),
    )
    .expect("search");
    assert_eq!(variadic.0[0].4[0].1, "1, 2, 3");

    let mut paged = search_request("same($A, $A)", &["sample.js"]);
    paged.offset = 1;
    paged.limit = 1;
    let paged = search(&canonical(&root), paged).expect("search");
    assert_eq!(paged.1, 2);
    assert_eq!(paged.0[0].2, "same(c, c)");
    assert!(!paged.5);
}

#[test]
fn search_reports_unsupported_parse_and_file_limits() {
    let root = workspace();
    fs::write(root.path().join("notes.txt"), "print(value)\n").expect("text file");
    fs::write(root.path().join("broken.py"), "def broken(\n").expect("broken source");
    let result = search(&canonical(&root), search_request("print($A)", &["."])).expect("search");
    assert_eq!(result.4, 1);
    assert_eq!(result.6[0].2, "parse_error");

    fs::write(root.path().join("other.py"), "print(1)\n").expect("second file");
    let mut request = search_request("print($A)", &["."]);
    request.limits.max_files = 1;
    assert!(matches!(
        search(&canonical(&root), request),
        Err(AstError::Limit(_))
    ));
}

#[test]
fn scanner_respects_hidden_gitignore_and_path_containment() {
    let root = workspace();
    fs::create_dir(root.path().join(".git")).expect("git directory");
    fs::write(root.path().join(".gitignore"), "ignored.py\n").expect("gitignore");
    fs::write(root.path().join("ignored.py"), "print(1)\n").expect("ignored file");
    fs::write(root.path().join(".hidden.py"), "print(2)\n").expect("hidden file");
    fs::write(root.path().join("visible.py"), "print(3)\n").expect("visible file");
    let result = search(&canonical(&root), search_request("print($A)", &["."])).expect("search");
    assert_eq!(result.1, 1);
    assert_eq!(result.0[0].0, "visible.py");

    let request = search_request("print($A)", &["../outside.py"]);
    assert!(matches!(
        search(&canonical(&root), request),
        Err(AstError::Path(_))
    ));
}

#[cfg(unix)]
#[test]
fn scanner_rejects_symlink_escape() {
    use std::os::unix::fs::symlink;

    let root = workspace();
    let outside = workspace();
    fs::write(outside.path().join("outside.py"), "print(1)\n").expect("outside file");
    symlink(
        outside.path().join("outside.py"),
        root.path().join("linked.py"),
    )
    .expect("symlink");
    let request = search_request("print($A)", &["linked.py"]);
    assert!(matches!(
        search(&canonical(&root), request),
        Err(AstError::Path(_))
    ));
}

#[test]
fn preview_deduplicates_edits_and_applies_in_reverse_order() {
    let root = workspace();
    let native = Workspace::from_canonical(&canonical(&root));
    fs::write(root.path().join("sample.py"), "print(1)\nprint(2)\n").expect("source file");
    let request = RewriteRequest {
        operations: vec![
            ("print($A)".to_owned(), "log($A)".to_owned()),
            ("print($A)".to_owned(), "log($A)".to_owned()),
        ],
        scan: scan(&["sample.py"]),
        language: Some("python".to_owned()),
        strictness: "smart".to_owned(),
        limits: limits(),
        cancellation: cancellation(),
    };
    let result = preview(&native, request).expect("preview");
    assert_eq!(result.3, 2);
    assert_eq!(result.1.len(), 2);
    assert_eq!(
        fs::read_to_string(root.path().join("sample.py")).expect("original"),
        "print(1)\nprint(2)\n"
    );

    let applied = apply(&native, result.0.inner, &cancellation()).expect("apply");
    assert_eq!(applied.1, 2);
    assert_eq!(
        fs::read_to_string(root.path().join("sample.py")).expect("updated"),
        "log(1)\nlog(2)\n"
    );
}

#[test]
fn preview_rejects_divergent_overlap_and_limits() {
    let root = workspace();
    let native = Workspace::from_canonical(&canonical(&root));
    fs::write(root.path().join("sample.py"), "print(1)\n").expect("source file");
    let mut request = RewriteRequest {
        operations: vec![
            ("print($A)".to_owned(), "log($A)".to_owned()),
            ("$F($A)".to_owned(), "trace($A)".to_owned()),
        ],
        scan: scan(&["sample.py"]),
        language: Some("python".to_owned()),
        strictness: "smart".to_owned(),
        limits: limits(),
        cancellation: cancellation(),
    };
    assert!(matches!(
        preview(&native, request.clone()),
        Err(AstError::Pattern(_))
    ));
    request.operations.pop();
    request.limits.max_replacements = 0;
    assert!(matches!(preview(&native, request), Err(AstError::Limit(_))));
}

#[test]
fn apply_rejects_stale_content_before_writing() {
    let root = workspace();
    let native = Workspace::from_canonical(&canonical(&root));
    fs::write(root.path().join("one.py"), "print(1)\n").expect("first source");
    fs::write(root.path().join("two.py"), "print(2)\n").expect("second source");
    let request = RewriteRequest {
        operations: vec![("print($A)".to_owned(), "log($A)".to_owned())],
        scan: scan(&["."]),
        language: Some("python".to_owned()),
        strictness: "smart".to_owned(),
        limits: limits(),
        cancellation: cancellation(),
    };
    let result = preview(&native, request).expect("preview");
    fs::write(root.path().join("two.py"), "print(3)\n").expect("stale source");
    assert!(matches!(
        apply(&native, result.0.inner, &cancellation()),
        Err(AstError::Stale(_))
    ));
    assert_eq!(
        fs::read_to_string(root.path().join("one.py")).expect("unchanged"),
        "print(1)\n"
    );
}

#[test]
fn proposals_are_bound_to_session_and_revision() {
    let root = workspace();
    fs::write(root.path().join("sample.py"), "print(1)\n").expect("source file");
    let root_value = root.path().to_string_lossy();
    let source = Workspace::with_id(&root_value, "source").expect("source workspace");
    let other = Workspace::with_id(&root_value, "other").expect("other workspace");
    let request = RewriteRequest {
        operations: vec![("print($A)".to_owned(), "log($A)".to_owned())],
        scan: scan(&["sample.py"]),
        language: Some("python".to_owned()),
        strictness: "smart".to_owned(),
        limits: limits(),
        cancellation: cancellation(),
    };
    let first = preview(&source, request.clone()).expect("first preview");
    assert!(matches!(
        apply(&other, first.0.inner.clone(), &cancellation()),
        Err(AstError::Configuration(_))
    ));

    let second = preview(&source, request).expect("second preview");
    apply(&source, first.0.inner, &cancellation()).expect("first apply");
    assert!(matches!(
        apply(&source, second.0.inner, &cancellation()),
        Err(AstError::Stale(_))
    ));
}

#[test]
fn cancellation_stops_search_before_work() {
    let root = workspace();
    fs::write(root.path().join("sample.py"), "print(1)\n").expect("source file");
    let cancellation = NativeAstCancellation::new();
    let mut request = search_request("print($A)", &["sample.py"]);
    request.cancellation = cancellation.token();
    cancellation.cancel();

    assert!(matches!(
        search(&canonical(&root), request),
        Err(AstError::Cancelled)
    ));
}

#[test]
fn strictness_mapping_accepts_the_public_contract() {
    for value in ["cst", "smart", "ast", "relaxed", "signature", "template"] {
        assert!(crate::ast::language::strictness(value).is_ok());
    }
    assert!(matches!(
        crate::ast::language::strictness("unknown"),
        Err(AstError::Configuration(_))
    ));
}
