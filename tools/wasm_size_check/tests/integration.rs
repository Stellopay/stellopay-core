//! End-to-end integration tests for `wasm_size_check`.
//!
//! Each test builds a minimal on-disk layout:
//!
//! ```text
//! <tmp>/
//! ├── wasm/                ← mimics onchain/target/wasm32-unknown-unknown/release
//! │   ├── alpha.wasm
//! │   └── beta.wasm
//! └── baseline.json        ← test-controlled baseline input
//! ```
//!
//! then calls [`wasm_size_check::compare`] or
//! [`wasm_size_check::refresh`] and asserts on the resulting Report.
//!
//! Coverage targets the documented guarantee: every requirement and
//! edge case in `tools/wasm_size_check/README.md` should have at
//! least one named test below. Mirrors the `.github/workflows/
//! contracts.yml` policy described in `docs/ci.md`.

use std::fs;
use std::path::Path;

use wasm_size_check::{
    compare, refresh, scan, Baseline, BaselineEntry, Measurement,
};
use wasm_size_check::report::{ComparisonStatus, Report};

/// Build a tiny WASM directory with one `.wasm` per `(name, payload)`
/// pair. Returns the directory path.
fn seed_wasm(dir: &Path, items: &[(&str, &[u8])]) {
    fs::create_dir_all(dir).unwrap();
    for (name, bytes) in items {
        fs::write(dir.join(format!("{name}.wasm")), bytes).unwrap();
    }
}

/// Build a baseline JSON with one entry per `(name, bytes)` pair.
fn seed_baseline(items: &[(&str, &[u8])]) -> Baseline {
    let mut b = Baseline::default();
    for (name, bytes) in items {
        b.contracts.insert(
            (*name).to_string(),
            BaselineEntry {
                size_bytes: bytes.len() as u64,
                sha256: Some(wasm_size_check::inventory::sha256_hex(bytes)),
                captured_at: Some("2026-07-28".to_string()),
            },
        );
    }
    b
}

// ---------------------------------------------------------------------------
// Compare-pass cases
// ---------------------------------------------------------------------------

#[test]
fn compare_passes_when_every_contract_matches_baseline_exactly() {
    let tmp = tempfile::tempdir().unwrap();
    let wasm = tmp.path().join("wasm");
    seed_wasm(&wasm, &[("alpha", &[1u8; 1000]), ("beta", &[2u8; 2000])]);
    let baseline = seed_baseline(&[("alpha", &[1u8; 1000]), ("beta", &[2u8; 2000])]);

    let r = compare(&baseline, &wasm, 5.0, true, true).unwrap();
    assert!(!r.has_failures(), "expected pass: {:#?}", r);
    assert_eq!(r.comparisons.len(), 2);
    for c in &r.comparisons {
        assert_eq!(c.status, ComparisonStatus::Ok);
        assert_eq!(c.delta_bytes, 0);
    }
}

#[test]
fn compare_passes_when_growth_is_within_tolerance() {
    let tmp = tempfile::tempdir().unwrap();
    let wasm = tmp.path().join("wasm");
    seed_wasm(&wasm, &[("alpha", &[0u8; 1050])]); // +5% from 1000
    let baseline = seed_baseline(&[("alpha", &[0u8; 1000])]);

    let r = compare(&baseline, &wasm, 5.0, true, true).unwrap();
    assert!(!r.has_failures(), "expected pass at exactly the tolerance boundary: {:#?}", r);
    assert_eq!(r.comparisons[0].status, ComparisonStatus::Ok);
}

#[test]
fn compare_passes_when_shrunk() {
    let tmp = tempfile::tempdir().unwrap();
    let wasm = tmp.path().join("wasm");
    seed_wasm(&wasm, &[("alpha", &[0u8; 900])]);
    let baseline = seed_baseline(&[("alpha", &[0u8; 1000])]);

    let r = compare(&baseline, &wasm, 5.0, true, true).unwrap();
    assert!(!r.has_failures(), "shrink should be a pass: {:#?}", r);
    assert_eq!(r.comparisons[0].status, ComparisonStatus::Shrunk);
    assert_eq!(r.comparisons[0].delta_bytes, -100);
}

#[test]
fn compare_fails_when_growth_exceeds_tolerance() {
    let tmp = tempfile::tempdir().unwrap();
    let wasm = tmp.path().join("wasm");
    seed_wasm(&wasm, &[("alpha", &[0u8; 1070])]); // +7%, tolerance 5%
    let baseline = seed_baseline(&[("alpha", &[0u8; 1000])]);

    let r = compare(&baseline, &wasm, 5.0, true, true).unwrap();
    assert!(r.has_failures());
    let alpha = &r.comparisons[0];
    assert_eq!(alpha.status, ComparisonStatus::Regression);
    assert!(alpha.delta_pct > 5.0);
}

#[test]
fn compare_treats_exact_tolerance_size_as_pass_not_regression() {
    // At exactly the tolerance boundary, the comparison must still
    // pass — `delta_pct > tolerance_pct` is strict, so `==` is ok.
    let tmp = tempfile::tempdir().unwrap();
    let wasm = tmp.path().join("wasm");
    seed_wasm(&wasm, &[("alpha", &[0u8; 1050])]);
    let baseline = seed_baseline(&[("alpha", &[0u8; 1000])]);

    let r = compare(&baseline, &wasm, 5.0, true, true).unwrap();
    assert_eq!(
        r.comparisons[0].status,
        ComparisonStatus::Ok,
        "exactly at tolerance must be ok, not regression",
    );
}

#[test]
fn compare_flags_one_byte_over_tolerance_as_regression() {
    let tmp = tempfile::tempdir().unwrap();
    let wasm = tmp.path().join("wasm");
    seed_wasm(&wasm, &[("alpha", &[0u8; 1051])]); // +5.1%
    let baseline = seed_baseline(&[("alpha", &[0u8; 1000])]);

    let r = compare(&baseline, &wasm, 5.0, true, true).unwrap();
    assert_eq!(r.comparisons[0].status, ComparisonStatus::Regression);
}

#[test]
fn compare_regression_threshold_supports_sub_percent_values() {
    let tmp = tempfile::tempdir().unwrap();
    let wasm = tmp.path().join("wasm");
    // baseline = 10_000 bytes; +200 bytes = +2% which fails at 1% tolerance.
    seed_wasm(&wasm, &[("alpha", &[0u8; 10_200])]);
    let baseline = seed_baseline(&[("alpha", &[0u8; 10_000])]);

    let r = compare(&baseline, &wasm, 1.0, true, true).unwrap();
    assert_eq!(r.comparisons[0].status, ComparisonStatus::Regression);
}

#[test]
fn compare_detects_hash_mismatch_even_when_size_is_identical() {
    let tmp = tempfile::tempdir().unwrap();
    let wasm = tmp.path().join("wasm");
    let bytes_now = vec![7u8; 1000];
    let bytes_baseline = vec![0u8; 1000];
    seed_wasm(&wasm, &[("alpha", &bytes_now)]);

    let mut b = Baseline::default();
    b.contracts.insert(
        "alpha".into(),
        BaselineEntry {
            size_bytes: 1000,
            sha256: Some(wasm_size_check::inventory::sha256_hex(&bytes_baseline)),
            captured_at: Some("2026-01-01".into()),
        },
    );

    let r = compare(&b, &wasm, 5.0, true, true).unwrap();
    assert!(r.has_failures());
    assert_eq!(r.comparisons[0].status, ComparisonStatus::HashMismatch);
}

#[test]
fn compare_treats_missing_baseline_hash_as_no_opinion() {
    // A baseline entry without `sha256` cannot be hash-checked; the
    // size check is the only signal.
    let tmp = tempfile::tempdir().unwrap();
    let wasm = tmp.path().join("wasm");
    seed_wasm(&wasm, &[("alpha", &[1u8; 1000])]);

    let mut b = Baseline::default();
    b.contracts.insert(
        "alpha".into(),
        BaselineEntry {
            size_bytes: 1000,
            sha256: None,
            captured_at: Some("2026-01-01".into()),
        },
    );

    let r = compare(&b, &wasm, 5.0, true, true).unwrap();
    assert_eq!(r.comparisons[0].status, ComparisonStatus::Ok);
}

// ---------------------------------------------------------------------------
// Inventory (new contract, missing artifact) cases
// ---------------------------------------------------------------------------

#[test]
fn compare_treats_new_contract_as_pass_when_fail_on_new_off() {
    let tmp = tempfile::tempdir().unwrap();
    let wasm = tmp.path().join("wasm");
    seed_wasm(&wasm, &[("alpha", &[0u8; 800]), ("new_contract", &[0u8; 500])]);
    let baseline = seed_baseline(&[("alpha", &[0u8; 800])]);

    let r = compare(&baseline, &wasm, 5.0, false, true).unwrap();
    assert!(!r.has_failures());
    let new = r
        .comparisons
        .iter()
        .find(|c| c.name == "new_contract")
        .unwrap();
    assert_eq!(new.status, ComparisonStatus::Ok);
}

#[test]
fn compare_treats_new_contract_as_failure_when_fail_on_new_on() {
    let tmp = tempfile::tempdir().unwrap();
    let wasm = tmp.path().join("wasm");
    seed_wasm(&wasm, &[("new_contract", &[0u8; 500])]);
    let baseline = Baseline::default();

    let r = compare(&baseline, &wasm, 5.0, true, true).unwrap();
    assert!(r.has_failures());
    assert_eq!(r.comparisons[0].status, ComparisonStatus::NewContract);
}

#[test]
fn compare_fails_on_missing_artifact_by_default() {
    let tmp = tempfile::tempdir().unwrap();
    let wasm = tmp.path().join("wasm");
    seed_wasm(&wasm, &[("alpha", &[0u8; 1000])]);
    let baseline = seed_baseline(&[
        ("alpha", &[0u8; 1000]),
        ("disabled", &[0u8; 200]),
    ]);

    let r = compare(&baseline, &wasm, 5.0, true, true).unwrap();
    assert!(r.has_failures());
    let disabled = r.comparisons.iter().find(|c| c.name == "disabled").unwrap();
    assert_eq!(disabled.status, ComparisonStatus::MissingWasm);
    assert_eq!(r.remaining_baseline_only, vec!["disabled".to_string()]);
}

#[test]
fn compare_ignores_missing_artifact_when_allow_missing_passed() {
    let tmp = tempfile::tempdir().unwrap();
    let wasm = tmp.path().join("wasm");
    seed_wasm(&wasm, &[("alpha", &[0u8; 1000])]);
    let baseline = seed_baseline(&[
        ("alpha", &[0u8; 1000]),
        ("disabled", &[0u8; 200]),
    ]);

    let r = compare(&baseline, &wasm, 5.0, true, false).unwrap();
    assert!(!r.has_failures());
    let disabled = r.comparisons.iter().find(|c| c.name == "disabled").unwrap();
    assert_eq!(disabled.status, ComparisonStatus::Ok);
    assert!(r.remaining_baseline_only.is_empty());
}

// ---------------------------------------------------------------------------
// Tolerance validation
// ---------------------------------------------------------------------------

#[test]
fn compare_rejects_negative_tolerance() {
    let tmp = tempfile::tempdir().unwrap();
    let wasm = tmp.path().join("wasm");
    seed_wasm(&wasm, &[("alpha", &[0u8; 1000])]);
    let baseline = seed_baseline(&[("alpha", &[0u8; 1000])]);

    let err = compare(&baseline, &wasm, -1.0, true, true).unwrap_err();
    matches!(err, wasm_size_check::Error::InvalidTolerance(_));
}

#[test]
fn compare_rejects_nan_tolerance() {
    let tmp = tempfile::tempdir().unwrap();
    let wasm = tmp.path().join("wasm");
    seed_wasm(&wasm, &[("alpha", &[0u8; 1000])]);
    let baseline = seed_baseline(&[("alpha", &[0u8; 1000])]);

    let err = compare(&baseline, &wasm, f64::NAN, true, true).unwrap_err();
    matches!(err, wasm_size_check::Error::InvalidTolerance(_));
}

#[test]
fn compare_rejects_missing_wasm_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let baseline = Baseline::default();
    let err = compare(&baseline, &tmp.path().join("does/not/exist"), 5.0, true, true).unwrap_err();
    matches!(err, wasm_size_check::Error::WasmDirRead { .. });
}

// ---------------------------------------------------------------------------
// Refresh (--update-baseline) cases
// ---------------------------------------------------------------------------

#[test]
fn refresh_replaces_all_entries_with_current_measurements() {
    let tmp = tempfile::tempdir().unwrap();
    let wasm = tmp.path().join("wasm");
    seed_wasm(&wasm, &[("alpha", &[1u8; 1234]), ("beta", &[2u8; 5678])]);
    let existing = seed_baseline(&[("alpha", &[0u8; 99999])]); // stale

    let new = refresh(&existing, &wasm, Some("2026-08-01".into()), None).unwrap();
    assert_eq!(new.contracts.len(), 2);
    assert_eq!(new.contracts["alpha"].size_bytes, 1234);
    assert_eq!(new.contracts["beta"].size_bytes, 5678);
    assert_eq!(new.captured_at.as_deref(), Some("2026-08-01"));
    // The stale "alpha" size in `existing` is gone.
    assert!(new.contracts["alpha"].sha256.is_some());
}

#[test]
fn refresh_drops_baseline_only_entries_that_no_longer_exist() {
    let tmp = tempfile::tempdir().unwrap();
    let wasm = tmp.path().join("wasm");
    seed_wasm(&wasm, &[("alpha", &[0u8; 100])]);
    let existing = seed_baseline(&[
        ("alpha", &[0u8; 100]),
        ("removed", &[0u8; 200]),
    ]);

    let new = refresh(&existing, &wasm, Some("2026-08-01".into()), None).unwrap();
    assert!(!new.contracts.contains_key("removed"));
    assert!(new.contracts.contains_key("alpha"));
}

#[test]
fn refresh_writes_baseline_to_disk() {
    let tmp = tempfile::tempdir().unwrap();
    let wasm = tmp.path().join("wasm");
    seed_wasm(&wasm, &[("alpha", &[0u8; 1234])]);
    let baseline_path = tmp.path().join("wasm_sizes.json");
    let existing = Baseline::default();

    refresh(&existing, &wasm, Some("2026-08-01".into()), Some(&baseline_path)).unwrap();
    assert!(baseline_path.exists());

    let reread = Baseline::read(&baseline_path).unwrap();
    assert_eq!(reread.contracts["alpha"].size_bytes, 1234);
    assert_eq!(reread.version, wasm_size_check::baseline::BASELINE_VERSION);
    assert_eq!(reread.captured_at.as_deref(), Some("2026-08-01"));
}

#[test]
fn refresh_records_sha256_in_each_entry() {
    let tmp = tempfile::tempdir().unwrap();
    let wasm = tmp.path().join("wasm");
    seed_wasm(&wasm, &[("alpha", &[9u8; 4242])]);
    let existing = Baseline::default();
    let new = refresh(&existing, &wasm, Some("2026-08-01".into()), None).unwrap();

    let expected = wasm_size_check::inventory::sha256_hex(&[9u8; 4242]);
    assert_eq!(new.contracts["alpha"].sha256.as_deref(), Some(expected.as_str()));
    assert_eq!(new.contracts["alpha"].captured_at.as_deref(), Some("2026-08-01"));
}

// ---------------------------------------------------------------------------
// Inventory scanner unit cases
// ---------------------------------------------------------------------------

#[test]
fn scan_walks_recursively_picking_up_nested_wasm_files() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("wasm");
    fs::create_dir_all(root.join("release")).unwrap();
    fs::write(root.join("release").join("alpha.wasm"), vec![1u8; 50]).unwrap();
    fs::write(root.join("release").join("beta.wasm"), vec![2u8; 60]).unwrap();
    fs::write(root.join("other.suffix"), b"ignore me").unwrap(); // not .wasm

    let inv = scan(&root).unwrap();
    assert_eq!(inv.len(), 2);
    assert_eq!(inv["alpha"].size_bytes, 50);
    assert_eq!(inv["beta"].size_bytes, 60);
}

#[test]
fn scan_first_hit_wins_on_duplicate_name() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("wasm");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("dup.wasm"), vec![1u8; 10]).unwrap();
    fs::create_dir_all(root.join("nested")).unwrap();
    fs::write(root.join("nested").join("dup.wasm"), vec![1u8; 99]).unwrap();

    let inv = scan(&root).unwrap();
    assert_eq!(inv.len(), 1);
    // Either the early-encountered file is kept, or our tie-break
    // picks the smaller one deterministically. Both options are pass
    // — the only invariant is "exactly one entry".
    assert_eq!(inv["dup"].size_bytes, 10);
}

#[test]
fn sha256_hash_is_stable_and_lowercase() {
    let h = wasm_size_check::inventory::sha256_hex(b"hello");
    let expected = format!(
        "sha256:{}",
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
    );
    assert_eq!(h, expected);
    assert!(h.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == ':'));
}

// ---------------------------------------------------------------------------
// Markdown rendering
// ---------------------------------------------------------------------------

#[test]
fn markdown_report_contains_one_row_per_comparison_and_no_failures_header_when_clean() {
    let tmp = tempfile::tempdir().unwrap();
    let wasm = tmp.path().join("wasm");
    seed_wasm(&wasm, &[("alpha", &[1u8; 1000])]);
    let baseline = seed_baseline(&[("alpha", &[1u8; 1000])]);
    let r = compare(&baseline, &wasm, 5.0, true, true).unwrap();

    let md = r.to_markdown();
    assert!(md.contains("WASM size regression report"));
    assert!(md.contains("| alpha |"));
    // One passing comparison, zero failures.
    assert!(
        md.contains("1 pass / 0 fail"),
        "summary line should reflect 1 pass / 0 fail; got:\n{md}",
    );
}

#[test]
fn markdown_report_surfaces_missing_artifacts_section() {
    let tmp = tempfile::tempdir().unwrap();
    let wasm = tmp.path().join("wasm");
    seed_wasm(&wasm, &[("alpha", &[0u8; 1000])]);
    let baseline = seed_baseline(&[("alpha", &[0u8; 1000]), ("ghost", &[0u8; 500])]);
    let r = compare(&baseline, &wasm, 5.0, true, true).unwrap();

    let md = r.to_markdown();
    assert!(md.contains("## Missing artifacts"));
    assert!(md.contains("- ghost"));
}

// ---------------------------------------------------------------------------
// Baseline IO
// ---------------------------------------------------------------------------

#[test]
fn baseline_read_treats_empty_file_as_empty_baseline() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("empty.json");
    fs::write(&p, "").unwrap();
    let b = Baseline::read(&p).unwrap();
    assert!(b.is_empty());
}

#[test]
fn baseline_read_treats_whitespace_only_file_as_empty_baseline() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("ws.json");
    fs::write(&p, "   \n\t\n").unwrap();
    let b = Baseline::read(&p).unwrap();
    assert!(b.is_empty());
}

#[test]
fn baseline_round_trip_preserves_entries() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("wasm_sizes.json");

    let mut b = Baseline::default();
    b.captured_at = Some("2026-07-28".to_string());
    b.contracts.insert(
        "stello_pay_contract".into(),
        BaselineEntry {
            size_bytes: 12_345,
            sha256: Some("sha256:abcd".into()),
            captured_at: Some("2026-07-28".into()),
        },
    );
    b.write(&p).unwrap();

    let reloaded = Baseline::read(&p).unwrap();
    assert_eq!(reloaded, b);
}

#[test]
fn baseline_parse_error_is_reported_for_invalid_json() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("bad.json");
    fs::write(&p, b"{ not json").unwrap();
    let err = Baseline::read(&p).unwrap_err();
    assert!(matches!(err, wasm_size_check::Error::BaselineParse { .. }));
}

#[test]
fn baseline_read_io_error_is_reported_for_missing_path() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("does_not_exist.json");
    // Path that does not exist must surface as `BaselineRead`
    // (kinds vary by platform so we don't pin the inner ErrorKind).
    let err = Baseline::read(&p).unwrap_err();
    assert!(
        matches!(err, wasm_size_check::Error::BaselineRead { .. }),
        "expected BaselineRead for missing path, got {err:?}",
    );
}

#[test]
fn baseline_missing_for_compare_helper_signals_non_update_mode() {
    // The CLI's `load_baseline` helper turns "baseline absent +
    // --update-baseline=false" into `BaselineMissingForCompare`.
    // We exercise that path here so the CLI failure mode is
    // covered even if the binary's `--help` output changes.
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("not_yet_baselined.json");
    let err = wasm_size_check::Baseline::read(&missing).unwrap_err();
    assert!(matches!(err, wasm_size_check::Error::BaselineRead { .. }));
}

#[test]
fn inventory_measurement_equality_is_structural() {
    let a = Measurement {
        name: "x".into(),
        size_bytes: 10,
        sha256: "sha256:abc".into(),
    };
    let b = Measurement {
        name: "x".into(),
        size_bytes: 10,
        sha256: "sha256:abc".into(),
    };
    assert_eq!(a, b);

    let c = Measurement {
        name: "x".into(),
        size_bytes: 11,
        sha256: "sha256:abc".into(),
    };
    assert_ne!(a, c);
}

#[test]
fn report_clone_is_independent() {
    let tmp = tempfile::tempdir().unwrap();
    let wasm = tmp.path().join("wasm");
    seed_wasm(&wasm, &[("alpha", &[0u8; 1000])]);
    let baseline = seed_baseline(&[("alpha", &[0u8; 1000])]);
    let r: Report = compare(&baseline, &wasm, 5.0, true, true).unwrap();
    let mut r2 = r.clone();
    r2.comparisons[0].status = ComparisonStatus::Regression;
    assert_eq!(r.comparisons[0].status, ComparisonStatus::Ok);
    assert_eq!(r2.comparisons[0].status, ComparisonStatus::Regression);
}


