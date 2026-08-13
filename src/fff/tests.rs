use std::fs;

use tempfile::TempDir;

use crate::fff::engine::NativeFffEngine;
use crate::fff::find::find;
use crate::fff::grep::{grep, multi_grep};
use crate::fff::types::{
    FffConfig, FffConstraints, FffFindRequest, FffGrepRequest, FffLimits, FffMultiGrepRequest,
};

fn engine(root: &str) -> NativeFffEngine {
    NativeFffEngine::new(
        root.to_owned(),
        FffConfig {
            watch: false,
            enable_content_indexing: true,
            enable_mmap_cache: false,
            initial_scan_timeout_seconds: 10.0,
            search_timeout_seconds: 5.0,
        },
        FffLimits {
            max_results: 200,
            max_matches_per_file: 100,
            max_patterns: 32,
            max_pattern_characters: 1_000,
            max_query_characters: 2_000,
            max_file_bytes: 10 * 1024 * 1024,
            max_context_lines: 10,
            max_search_timeout_seconds: 30.0,
        },
    )
    .unwrap()
}

fn constraints() -> FffConstraints {
    FffConstraints {
        include: vec![],
        exclude: vec![],
        git_status: None,
    }
}

#[test]
fn indexes_and_searches_paths_and_content() {
    let directory = TempDir::new().unwrap();
    fs::write(
        directory.path().join("credential_resolver.py"),
        "class CredentialResolver:\n    pass\n",
    )
    .unwrap();
    let engine = engine(directory.path().to_str().unwrap());
    engine.inner.wait_ready(10.0).unwrap();

    let found = find(
        &engine.inner,
        FffFindRequest {
            query: "credentail resolver".to_owned(),
            constraints: constraints(),
            kind: "file".to_owned(),
            offset: 0,
            limit: 20,
        },
    )
    .unwrap();
    assert_eq!(found.0[0].0, "credential_resolver.py");

    let result = grep(
        &engine.inner,
        FffGrepRequest {
            query: "CredentialResolver".to_owned(),
            constraints: constraints(),
            mode: "plain".to_owned(),
            smart_case: true,
            file_offset: 0,
            limit: 20,
            matches_per_file: 10,
            context_before: 0,
            context_after: 0,
            max_file_bytes: 1024,
            timeout_seconds: 5.0,
            classify_definitions: true,
        },
        Default::default(),
    )
    .unwrap();
    assert_eq!(result.0[0].0, "credential_resolver.py");
    assert!(result.0[0].9);
}

#[test]
fn multi_grep_treats_patterns_as_literals() {
    let directory = TempDir::new().unwrap();
    fs::write(
        directory.path().join("names.txt"),
        "credential_resolver\nCredentialResolver\na+b\n",
    )
    .unwrap();
    let engine = engine(directory.path().to_str().unwrap());
    engine.inner.wait_ready(10.0).unwrap();

    let result = multi_grep(
        &engine.inner,
        FffMultiGrepRequest {
            patterns: vec!["credential_resolver".to_owned(), "a+b".to_owned()],
            constraints: constraints(),
            smart_case: true,
            file_offset: 0,
            limit: 20,
            matches_per_file: 10,
            context_before: 0,
            context_after: 0,
            max_file_bytes: 1024,
            timeout_seconds: 5.0,
            classify_definitions: false,
        },
        Default::default(),
    )
    .unwrap();

    assert_eq!(result.0.len(), 2);
}
