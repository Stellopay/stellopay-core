//! Integration tests for the orphaned-`docs/*.md` reachability check.
//!
//! These tests exercise `doc_checker::find_orphaned_docs` /
//! `doc_checker::check_orphaned_docs` against small fixture "repos" under
//! `tests/fixtures/`, each with its own `README.md` and `docs/` tree, rather
//! than against this repository's real (much larger) `docs/` directory. That
//! keeps the assertions exact and independent of unrelated documentation
//! changes elsewhere in the repo.
//!
//! Security note: fixtures are static files checked into the repository and
//! read-only from the checker's point of view; no fixture path is ever
//! executed or interpreted as anything other than a markdown file to scan.

use doc_checker::{check_orphaned_docs, find_orphaned_docs, CheckConfig, Severity};
use std::path::{Path, PathBuf};

fn fixture_root(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn relative_strs(repo_root: &Path, paths: &[PathBuf]) -> Vec<String> {
    let mut out: Vec<String> = paths
        .iter()
        .map(|p| {
            p.strip_prefix(repo_root)
                .unwrap_or(p)
                .display()
                .to_string()
                .replace('\\', "/")
        })
        .collect();
    out.sort();
    out
}

#[test]
fn test_orphaned_doc_is_flagged() {
    let repo_root = fixture_root("orphaned_docs");
    let orphaned = find_orphaned_docs(&repo_root);
    let relative = relative_strs(&repo_root, &orphaned);

    assert_eq!(
        relative,
        vec!["docs/orphaned.md".to_string()],
        "expected exactly the intentionally-orphaned fixture file to be flagged, got: {:?}",
        relative
    );
}

#[test]
fn test_linked_docs_are_not_flagged() {
    let repo_root = fixture_root("orphaned_docs");
    let orphaned = find_orphaned_docs(&repo_root);
    let relative = relative_strs(&repo_root, &orphaned);

    for reachable in [
        "docs/README.md",
        "docs/linked.md",
        "docs/deep-linked.md",
        "docs/section/README.md",
        "docs/section/nested.md",
    ] {
        assert!(
            !relative.iter().any(|r| r == reachable),
            "{} should be reachable and must not be reported as orphaned (got: {:?})",
            reachable,
            relative
        );
    }
}

#[test]
fn test_nested_index_counts_as_entry_point() {
    // `docs/section/README.md` is not linked from README.md or docs/README.md
    // in the fixture, yet it must still seed traversal so `nested.md` (only
    // reachable through it) is not misreported as orphaned.
    let repo_root = fixture_root("orphaned_docs");
    let orphaned = find_orphaned_docs(&repo_root);
    let relative = relative_strs(&repo_root, &orphaned);
    assert!(!relative.iter().any(|r| r == "docs/section/nested.md"));
}

#[test]
fn test_missing_docs_dir_yields_no_findings() {
    let repo_root = fixture_root("orphaned_docs_no_docs_dir");
    let orphaned = find_orphaned_docs(&repo_root);
    assert!(
        orphaned.is_empty(),
        "a repo with no docs/ directory has nothing to flag, got: {:?}",
        orphaned
    );
}

#[test]
fn test_check_orphaned_docs_defaults_to_warn_severity() {
    let repo_root = fixture_root("orphaned_docs");
    let config = CheckConfig::default();
    let findings = check_orphaned_docs(&repo_root, &config);

    assert_eq!(findings.len(), 1, "got: {:?}", findings);
    assert_eq!(findings[0].severity, Severity::Warn);
    assert!(findings[0].message.contains("docs/orphaned.md"));
    assert!(findings[0].message.contains("orphaned"));
}

#[test]
fn test_check_orphaned_docs_strict_promotes_to_fail() {
    let repo_root = fixture_root("orphaned_docs");
    let config = CheckConfig {
        new_check_severity: Severity::Fail,
        ..CheckConfig::default()
    };
    let findings = check_orphaned_docs(&repo_root, &config);

    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].severity, Severity::Fail);
}

#[test]
fn test_check_orphaned_docs_can_be_disabled() {
    let repo_root = fixture_root("orphaned_docs");
    let config = CheckConfig {
        check_orphaned_docs: false,
        ..CheckConfig::default()
    };
    let findings = check_orphaned_docs(&repo_root, &config);
    assert!(findings.is_empty());
}
