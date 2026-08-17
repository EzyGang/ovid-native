use std::fs;

use tempfile::TempDir;

use crate::search::SearchError;
use crate::search::glob::glob;
use crate::search::grep::grep;
use crate::search::types::{GlobRequest, GrepRequest};
use crate::workspace::{Cancellation, Workspace};

fn workspace() -> TempDir {
    let root = tempfile::tempdir().expect("temporary workspace");
    fs::write(
        root.path().join("a.txt"),
        "zero\nneedle one\nafter\nneedle two\n",
    )
    .expect("write a");
    fs::write(root.path().join("b.txt"), "needle three\n").expect("write b");
    root
}

fn grep_request(pattern: &str) -> GrepRequest {
    GrepRequest {
        pattern: pattern.to_owned(),
        paths: vec![".".to_owned()],
        include_hidden: false,
        respect_gitignore: true,
        include_node_modules: false,
        mode: "regex".to_owned(),
        case_sensitive: true,
        multiline: false,
        file_offset: 0,
        file_limit: 20,
        matches_per_file: 20,
        context_before: 0,
        context_after: 0,
        max_file_bytes: 4 * 1024 * 1024,
        large_file_mode: "prefix".to_owned(),
        timeout_seconds: 30.0,
        max_scan_files: 10_000,
        max_grep_matches: 5_000,
        max_matches_per_file: 200,
        max_line_characters: 2_000,
        cancellation: Cancellation::new(),
    }
}

fn glob_request(patterns: &[&str]) -> GlobRequest {
    GlobRequest {
        patterns: patterns.iter().map(|value| (*value).to_owned()).collect(),
        include_hidden: false,
        respect_gitignore: true,
        include_node_modules: false,
        file_type: "any".to_owned(),
        order: "path".to_owned(),
        limit: 200,
        max_scan_files: 10_000,
        timeout_seconds: 5.0,
        cancellation: Cancellation::new(),
    }
}

#[test]
fn glob_discovers_files_directories_and_deduplicates_selections() {
    let root = workspace();
    fs::create_dir(root.path().join("src")).expect("src directory");
    fs::write(root.path().join("src/lib.rs"), "fn main() {}\n").expect("source");
    let workspace = Workspace::new(&root.path().to_string_lossy()).expect("workspace");

    let result = glob(&workspace, glob_request(&["src", "src/**/*.rs"])).expect("glob");
    let paths = result
        .0
        .iter()
        .map(|value| value.0.as_str())
        .collect::<Vec<_>>();

    assert_eq!(paths, ["src/", "src/lib.rs"]);
    assert_eq!(result.1, "complete");
    assert!(!result.4);
}

#[test]
fn glob_respects_ignore_hidden_and_result_limits() {
    let root = workspace();
    fs::write(root.path().join(".gitignore"), "ignored.txt\n").expect("ignore file");
    fs::write(root.path().join("ignored.txt"), "ignored\n").expect("ignored content");
    fs::write(root.path().join(".hidden.txt"), "hidden\n").expect("hidden content");
    let workspace = Workspace::new(&root.path().to_string_lossy()).expect("workspace");
    let mut request = glob_request(&["."]);
    request.limit = 1;

    let result = glob(&workspace, request).expect("glob");

    assert_eq!(result.0.len(), 1);
    assert!(result.4);
    assert!(
        result
            .0
            .iter()
            .all(|value| !value.0.contains("ignored") && !value.0.contains("hidden"))
    );
}

#[test]
fn grep_reports_positions_context_and_per_file_truncation() {
    let root = workspace();
    let workspace = Workspace::new(&root.path().to_string_lossy()).expect("workspace");
    let mut request = grep_request("needle");
    request.paths = vec!["a.txt".to_owned()];
    request.matches_per_file = 1;
    request.context_before = 1;
    request.context_after = 1;

    let result = grep(&workspace, request).expect("grep");
    let file = &result.0[0];
    let matched = &file.1[0];

    assert_eq!(matched.0, "needle");
    assert_eq!(matched.1.0, (2, 1, 5));
    assert_eq!(matched.4, [(2, "needle one".to_owned(), false)]);
    assert_eq!(matched.5, [(1, "zero".to_owned(), false)]);
    assert_eq!(matched.6, [(3, "after".to_owned(), false)]);
    assert_eq!(file.2, 2);
    assert!(file.3);
    assert!(file.4);
}

#[test]
fn grep_returns_every_complete_line_intersecting_a_multiline_match() {
    let root = workspace();
    let workspace = Workspace::new(&root.path().to_string_lossy()).expect("workspace");
    let mut request = grep_request("needle one.*needle two");
    request.paths = vec!["a.txt".to_owned()];
    request.multiline = true;

    let result = grep(&workspace, request).expect("grep");
    let matched = &result.0[0].1[0];

    assert_eq!(
        matched.4,
        [
            (2, "needle one".to_owned(), false),
            (3, "after".to_owned(), false),
            (4, "needle two".to_owned(), false),
        ]
    );
}

#[test]
fn grep_uses_pcre2_and_auto_literal_fallback() {
    let root = workspace();
    fs::write(root.path().join("pattern.txt"), "foobar\n(\n").expect("patterns");
    let workspace = Workspace::new(&root.path().to_string_lossy()).expect("workspace");
    let mut pcre = grep_request("foo(?=bar)");
    pcre.paths = vec!["pattern.txt".to_owned()];

    let pcre_result = grep(&workspace, pcre).expect("PCRE2 grep");
    assert_eq!(pcre_result.1, "pcre2");
    assert_eq!(pcre_result.0[0].1[0].0, "foo");

    let mut auto = grep_request("(");
    auto.paths = vec!["pattern.txt".to_owned()];
    auto.mode = "auto".to_owned();
    let auto_result = grep(&workspace, auto).expect("auto grep");
    assert_eq!(auto_result.1, "rust");
    assert!(auto_result.2);
    assert_eq!(auto_result.0[0].1[0].0, "(");
}

#[test]
fn grep_rejects_invalid_strict_regex() {
    let root = workspace();
    let workspace = Workspace::new(&root.path().to_string_lossy()).expect("workspace");

    let error = grep(&workspace, grep_request("(")).expect_err("invalid regex");

    assert!(matches!(error, SearchError::Pattern(_)));
}

#[test]
fn grep_paginates_matching_files_and_bounds_hot_files() {
    let root = workspace();
    let workspace = Workspace::new(&root.path().to_string_lossy()).expect("workspace");
    let mut first = grep_request("needle");
    first.file_limit = 1;
    first.matches_per_file = 1;

    let first_result = grep(&workspace, first).expect("first page");
    assert_eq!(first_result.0[0].0, "a.txt");
    assert_eq!(first_result.10, Some(1));
    assert!(first_result.11);

    let mut second = grep_request("needle");
    second.file_limit = 1;
    second.file_offset = 1;
    let second_result = grep(&workspace, second).expect("second page");
    assert_eq!(second_result.0[0].0, "b.txt");
}

#[test]
fn grep_reports_binary_encoding_and_large_file_coverage() {
    let root = workspace();
    fs::write(root.path().join("binary.bin"), b"nee\0dle").expect("binary");
    fs::write(root.path().join("encoding.txt"), b"nee\xFFdle").expect("encoding");
    fs::write(root.path().join("large.txt"), "needle trailing data").expect("large");
    fs::write(root.path().join("unicode.txt"), "needleé").expect("unicode");
    let workspace = Workspace::new(&root.path().to_string_lossy()).expect("workspace");
    let mut request = grep_request("needle");
    request.max_file_bytes = 6;

    let result = grep(&workspace, request).expect("grep");
    let large = result
        .0
        .iter()
        .find(|file| file.0 == "large.txt")
        .expect("large match");

    assert_eq!(result.7, 1);
    assert_eq!(result.8, 1);
    assert_eq!(large.5, (6, 20, false));
    assert!(result.11);

    let mut unicode_request = grep_request("needle");
    unicode_request.paths = vec!["unicode.txt".to_owned()];
    unicode_request.max_file_bytes = 7;
    let unicode = grep(&workspace, unicode_request).expect("Unicode prefix");
    assert_eq!(unicode.8, 0);
    assert_eq!(unicode.0[0].5, (6, 8, false));
}

#[test]
fn grep_skips_large_files_when_requested() {
    let root = workspace();
    fs::write(root.path().join("large.txt"), "needle trailing data").expect("large");
    let workspace = Workspace::new(&root.path().to_string_lossy()).expect("workspace");
    let mut request = grep_request("needle");
    request.max_file_bytes = 6;
    request.large_file_mode = "skip".to_owned();

    let result = grep(&workspace, request).expect("grep");

    assert_eq!(result.9, 3);
    assert!(result.0.is_empty());
}

#[test]
fn grep_supports_case_insensitive_and_multiline_matching() {
    let root = workspace();
    fs::write(root.path().join("multi.txt"), "Alpha\nbeta\n").expect("multiline");
    let workspace = Workspace::new(&root.path().to_string_lossy()).expect("workspace");
    let mut request = grep_request("alpha.*beta");
    request.paths = vec!["multi.txt".to_owned()];
    request.case_sensitive = false;
    request.multiline = true;

    let result = grep(&workspace, request).expect("grep");

    assert_eq!(result.0[0].1[0].0, "Alpha\nbeta");
}
