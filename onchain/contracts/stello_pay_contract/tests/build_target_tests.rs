//! `build_target_tests.rs` — sanity checks that the
//! `stello_pay_contract` crate is correctly configured for the
//! `wasm32-unknown-unknown` build target used by the workflow step in
//! `.github/workflows/contracts.yml` and by the `wasm_size_check`
//! regression tool.
//!
//! These tests do **not** compile to wasm themselves — they run on
//! the host's native toolchain as ordinary Rust integration tests.
//! Their job is to lock down the Cargo.toml declaration so an
//! accidental change (e.g. removal of `cdylib`, which is what allows
//! the crate to be linked into a deployable Soroban contract) is
//! caught by CI before the WASM build step can fail with a confusing
//! linker error.
//!
//! The assertions here stay text-based on purpose: pulling in a full
//! TOML parser as a `dev-dependency` would be unnecessary weight for
//! three lookups, and any non-trivial parser would itself become a
//! source of regressions to chase.

use std::path::PathBuf;

/// Resolve the Cargo manifest path of the contract under test.
/// `CARGO_MANIFEST_DIR` is set by Cargo for every test crate to the
/// directory containing its `Cargo.toml`.
fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")
}

/// Extract the `[lib]` block — the heading line plus its body until
/// the next section heading. This is intentionally a generous match
/// (allows whitespace, allows inline tables) so a hand-edited
/// Cargo.toml refactor doesn't accidentally break the test while
/// keeping the substantive declarations intact.
fn section(text: &str, header: &str) -> Option<String> {
    let start = text.find(&format!("[{}]", header))?;
    let rest = &text[start..];
    // Find the next section header, if any.
    let after_header_at = "[".len(); // skip past the leading "[header]"
    let body_start = start + after_header_at;
    let after = &text[body_start..];
    let body_end = after.find('\n[').map(|i| body_start + i).unwrap_or(text.len());
    Some(text[body_start..body_end].to_string())
}

/// Return `true` if the given section body contains the substring
/// inside double-quotes, surrounded by whitespace or punctuation that
/// would mark it as a distinct TOML array element.
fn array_contains_quote(body: &str, needle: &str) -> bool {
    // Match `"needle"` flanked by characters that would appear in a
    // TOML array: `,`, `[`, `]`, whitespace, or end of string.
    let n = needle.len();
    let bytes = body.as_bytes();
    needle.split('|').any(|part| {
        let m = part.len();
        for (i, _) in body.match_indices(part) {
            let before = if i == 0 { b'[' } else { bytes[i - 1] };
            let after = if i + m == bytes.len() { b']' } else { bytes[i + m] };
            if (before.is_ascii_whitespace() || before == b',' || before == b'[')
                && (after.is_ascii_whitespace() || after == b',' || after == b']')
            {
                return true;
            }
        }
        false
    }) || body.contains(&format!("\"{}\"", needle))
}

#[test]
fn crate_declares_cdylib_crate_type() {
    let manifest = std::fs::read_to_string(manifest_path()).expect("read Cargo.toml");
    let lib = section(&manifest, "lib").expect("crate must declare a [lib] section");

    assert!(
        array_contains_quote(&lib, "cdylib"),
        "stello_pay_contract must declare `cdylib` in [lib].crate-type \
         so `cargo build --target wasm32-unknown-unknown --release` \
         produces a deployable Soroban artifact. Current [lib] block:\n{}",
        lib,
    );
    // We also expect `rlib` so the contract can be consumed by
    // sibling integration tests in this workspace; assert by hand
    // rather than by feature flag because the latter is fragile.
    assert!(
        array_contains_quote(&lib, "rlib"),
        "stello_pay_contract must declare `rlib` in [lib].crate-type so that \
         sibling workspace crates can `use` the contract in tests.",
    );
}

#[test]
fn dependency_soroban_sdk_present() {
    let manifest = std::fs::read_to_string(manifest_path()).expect("read Cargo.toml");
    let deps = section(&manifest, "dependencies").expect("crate must declare [dependencies]");

    // Either `soroban-sdk` (the new name used in the workspace's
    // Cargo.lock) or `soroban_sdk` (the old underscore form) is
    // acceptable. We assert on either spelling so a rename on the
    // upstream crate doesn't trip our pin guards.
    let declared = deps.contains("soroban-sdk") || deps.contains("soroban_sdk");
    assert!(
        declared,
        "stello_pay_contract must depend on `soroban-sdk` (or `soroban_sdk`) \
         so its entrypoints can be hosted by the Soroban runtime. \
         Current [dependencies] block:\n{}",
        deps,
    );
}

#[test]
fn does_not_target_wasm32v1_none() {
    // `wasm32v1-none` is incompatible with the Soroban host. If a
    // contributor accidentally pins this target anywhere reachable
    // from the build, fail loudly here instead of producing a binary
    // the host will reject at deploy time.
    let manifest = std::fs::read_to_string(manifest_path()).expect("read Cargo.toml");
    assert!(
        !manifest.contains("wasm32v1-none"),
        "stello_pay_contract Cargo.toml must not reference the wasm32v1-none \
         target — only `wasm32-unknown-unknown` is supported by Soroban.",
    );
}

#[test]
fn manifest_version_present() {
    // Lock down the trivial invariants of the manifest so a vanishing
    // version or description field is caught explicitly.
    let manifest = std::fs::read_to_string(manifest_path()).expect("read Cargo.toml");
    let pkg = section(&manifest, "package").expect("crate must declare a [package] section");
    for required in &["name", "version", "edition"] {
        assert!(
            pkg.contains(required),
            "[package] must declare `{}` — got:\n{}",
            required,
            pkg,
        );
    }
}
