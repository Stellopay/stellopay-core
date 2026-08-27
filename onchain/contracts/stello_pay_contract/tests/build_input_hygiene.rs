//! Regression checks for the contract's build inputs.
//!
//! These checks intentionally inspect the manifests rather than the compiled
//! crate. The wasm build is performed by the release workflow; these tests
//! make the two easy-to-regress source-level guarantees explicit in the normal
//! test suite.

use std::{fs, path::PathBuf};

fn onchain_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("contract must remain under onchain/contracts")
        .to_path_buf()
}

fn contract_manifest() -> String {
    fs::read_to_string(env!("CARGO_MANIFEST_DIR").to_owned() + "/Cargo.toml")
        .expect("contract manifest must be readable")
}

#[test]
fn removed_dependencies_are_not_declared_again() {
    let manifest = contract_manifest();
    for dependency in [
        "soroban-token-sdk",
        "stellar-access",
        "stellar-contract-utils",
        "stellar-macros",
        "stellar-tokens",
    ] {
        assert!(
            !manifest
                .lines()
                .any(|line| { line.trim_start().starts_with(&format!("{dependency} =")) }),
            "unused production dependency {dependency} was reintroduced"
        );
    }
}

#[test]
fn removed_dependencies_are_not_in_the_workspace_lockfile() {
    let lockfile = fs::read_to_string(onchain_root().join("Cargo.lock"))
        .expect("workspace lockfile must be readable");
    for package in [
        "soroban-token-sdk",
        "stellar-access",
        "stellar-contract-utils",
        "stellar-macros",
        "stellar-tokens",
    ] {
        let package_header = format!("name = \"{package}\"");
        assert!(
            !lockfile.lines().any(|line| line.trim() == package_header),
            "unused package {package} was reintroduced to Cargo.lock"
        );
    }
}

#[test]
fn mock_contract_is_test_only() {
    let source = fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs"))
        .expect("contract source must be readable");
    let gate = "#[cfg(test)]\npub mod mock_contract;";
    assert!(
        source.contains(gate),
        "mock_contract must remain behind the cfg(test) production-build gate"
    );
}

#[test]
fn integration_hook_support_stays_outside_the_library() {
    let support = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/support/mod.rs");
    let source = fs::read_to_string(support).expect("integration support must exist");
    assert!(source.contains("pub struct MaliciousMilestoneHook"));
    assert!(source.contains("#[contractimpl]"));
}
