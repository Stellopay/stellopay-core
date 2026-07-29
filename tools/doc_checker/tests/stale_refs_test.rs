//! Integration tests for the stale doc reference check.
//!
//! These tests exercise `doc_checker::check_stale_doc_references` and
//! `doc_checker::find_stale_references` against small fixture "repos" under
//! `tests/fixtures/`, each with its own `docs/` and `contracts/` tree.

use doc_checker::{check_stale_doc_references, find_stale_references, CheckConfig, Severity};
use std::path::{Path, PathBuf};

fn fixture_root(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn test_valid_refs_no_findings() {
    let repo_root = fixture_root("valid_refs");
    let stale = find_stale_references(
        &repo_root.join("docs"),
        &repo_root.join("contracts"),
    );
    assert!(
        stale.is_empty(),
        "all doc references should match contract functions, got: {:?}",
        stale
    );
}

#[test]
fn test_stale_refs_are_detected() {
    let repo_root = fixture_root("stale_refs");
    let stale = find_stale_references(
        &repo_root.join("docs"),
        &repo_root.join("contracts"),
    );
    // stale_refs/docs/guide.md has:
    //   `old_deprecated_func` (line 11)
    //   `never_existed` (line 12)
    //   `legacy_limit_lookup` (line 13)
    assert_eq!(stale.len(), 3, "got: {:?}", stale);

    let names: Vec<&str> = stale.iter().map(|(_, n, _)| n.as_str()).collect();
    let lines: Vec<usize> = stale.iter().map(|(_, _, l)| *l).collect();

    assert!(names.contains(&"old_deprecated_func"));
    assert!(names.contains(&"never_existed"));
    assert!(names.contains(&"legacy_limit_lookup"));
    assert!(lines.contains(&11));
    assert!(lines.contains(&12));
    assert!(lines.contains(&13));
}

#[test]
fn test_check_stale_doc_references_default_severity() {
    let repo_root = fixture_root("stale_refs");
    let config = CheckConfig::default();
    let findings = check_stale_doc_references(&repo_root, &config);

    assert_eq!(findings.len(), 3, "got: {:?}", findings);
    for f in &findings {
        assert_eq!(f.severity, Severity::Warn);
    }
    assert!(findings[0].message.contains("old_deprecated_func"));
    assert!(findings[0].message.contains("stale doc reference"));
}

#[test]
fn test_check_stale_doc_references_can_be_disabled() {
    let repo_root = fixture_root("stale_refs");
    let config = CheckConfig {
        check_stale_refs: false,
        ..CheckConfig::default()
    };
    let findings = check_stale_doc_references(&repo_root, &config);
    assert!(findings.is_empty());
}

#[test]
fn test_check_stale_doc_references_missing_docs_dir() {
    let repo_root = fixture_root("orphaned_docs_no_docs_dir");
    let config = CheckConfig::default();
    let findings = check_stale_doc_references(&repo_root, &config);
    assert!(findings.is_empty());
}
