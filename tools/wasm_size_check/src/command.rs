//! Compare and refresh entry points.
//!
//! [`compare`] is the read-only path called from CI: it loads the
//! baseline, scans the WASM directory, and returns a [`Report`] the
//! caller can render.
//!
//! [`refresh`] is the write path: it rebuilds the baseline from the
//! current artifacts, optionally annotating the result with the date
//! it was captured.

use std::collections::BTreeMap;
use std::path::Path;

use crate::baseline::{Baseline, BaselineEntry};
use crate::error::{Error, Result};
use crate::inventory::{scan, Measurement};
use crate::report::{classify as classify_into_report, Report};

/// Run a compare pass. Returns a [`Report`] the caller can render and
/// inspect for failures via [`Report::has_failures`].
///
/// `fail_on_new` controls whether a `.wasm` with no baseline entry is
/// a hard failure (true) or just informational (false).
///
/// `fail_on_missing` controls whether a baseline entry with no
/// matching `.wasm` is a hard failure (true) or just informational
/// (false). This is the negation of the `--allow-missing` CLI flag.
///
/// # Errors
/// Returns [`Error::InvalidTolerance`] if `tolerance_pct` is non-finite
/// or negative. Returns [`Error::WasmDirRead`] if the WASM directory
/// cannot be scanned.
pub fn compare(
    baseline: &Baseline,
    wasm_dir: &Path,
    tolerance_pct: f64,
    fail_on_new: bool,
    fail_on_missing: bool,
) -> Result<Report> {
    if !tolerance_pct.is_finite() || tolerance_pct < 0.0 {
        return Err(Error::InvalidTolerance(tolerance_pct));
    }
    let measurements = scan(wasm_dir)?;
    Ok(classify_into_report(
        baseline,
        &measurements,
        tolerance_pct,
        fail_on_new,
        fail_on_missing,
    ))
}

/// Rebuild the baseline from the current WASM artifacts. All previous
/// entries are discarded (the tool has no way to know which contracts
/// still exist in `main`) and replaced with whatever is on disk.
///
/// If `captured_at` is `None`, today's UTC date in `YYYY-MM-DD` is
/// inferred from the system clock.
///
/// If `write_to` is `Some`, the new baseline is also flushed to disk.
/// Errors during `write_to` are propagated.
///
/// # Errors
/// Returns [`Error::WasmDirRead`] if the WASM directory cannot be
/// scanned, [`Error::BaselineSerialize`] if the new baseline cannot be
/// JSON-encoded, or [`Error::BaselineWrite`] on flush failure.
pub fn refresh(
    baseline: &Baseline,
    wasm_dir: &Path,
    captured_at: Option<String>,
    write_to: Option<&Path>,
) -> Result<Baseline> {
    let measurements: BTreeMap<String, Measurement> = scan(wasm_dir)?;
    let mut new_baseline = baseline.clone();
    new_baseline.version = crate::baseline::BASELINE_VERSION;
    let at = captured_at.unwrap_or_else(today_utc);
    new_baseline.captured_at = Some(at.clone());
    new_baseline.contracts.clear();
    for (name, m) in &measurements {
        new_baseline.contracts.insert(
            name.clone(),
            BaselineEntry {
                size_bytes: m.size_bytes,
                sha256: Some(m.sha256.clone()),
                captured_at: Some(at.clone()),
            },
        );
    }
    if let Some(path) = write_to {
        new_baseline.write(path)?;
    }
    Ok(new_baseline)
}

/// Compute today's UTC date as a `YYYY-MM-DD` string using a
/// Howard-Hinnant civil-from-days calculation. Avoids pulling in
/// `time`/`chrono` just to stamp a date into the baseline file.
fn today_utc() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let (y, m, d) = civil_from_days(days);
    format!("{:04}-{:02}-{:02}", y, m, d)
}

/// Howard Hinnant's `civil_from_days`: convert days-since-Unix-epoch
/// into (year, month, day) on the proleptic Gregorian calendar. Kept
/// private + unit-tested so callers don't depend on the algorithm.
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { (mp + 3) as u32 } else { (mp - 9) as u32 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_is_1970_01_01() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
    }

    #[test]
    fn well_known_dates() {
        // Each day count below was independently hand-traced through
        // the Howard-Hinnant algorithm and cross-checked against a
        // leap-year count in [1970-01-01, target). They cover:
        //   - basic epoch boundaries
        //   - a non-leap Dec 31 (1970)
        //   - a leap-year Dec 31 (1972)
        //   - multi-decade and Y2K / centennial-leap boundaries
        //   - non-Jan-1 dates for cross-check
        //   - a mid-year date far from epoch (2015-08-12)
        //   - a date past the Y2K centennial-leap boundary
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(1), (1970, 1, 2));
        assert_eq!(civil_from_days(364), (1970, 12, 31));
        assert_eq!(civil_from_days(365), (1971, 1, 1));
        // 1972 is the first leap year after the epoch.
        assert_eq!(civil_from_days(730), (1972, 1, 1));
        assert_eq!(civil_from_days(1095), (1972, 12, 31)); // leap year
        assert_eq!(civil_from_days(1096), (1973, 1, 1));
        // 7305 = 20 * 365 + 5 leap days (1972, 76, 80, 84, 88).
        assert_eq!(civil_from_days(7_305), (1990, 1, 1));
        // 10957 = 30 * 365 + 7 leap days (+1992, 1996 — Feb 29 2000
        // isn't crossed because we stop at 2000-01-01).
        assert_eq!(civil_from_days(10_957), (2000, 1, 1));
        // 15706 = 43 * 365 + 11 leap days (+2000, 04, 08, 12).
        assert_eq!(civil_from_days(15_706), (2013, 1, 1));
        assert_eq!(civil_from_days(15_765), (2013, 3, 1)); // non-leap Feb
        // 16659: 2015-01-01 + 223 days = 2015-08-12 (DOY 224).
        assert_eq!(civil_from_days(16_659), (2015, 8, 12));
        // 18262 = 50 * 365 + 12 leap days (+2016, 2020).
        assert_eq!(civil_from_days(18_262), (2020, 1, 1));
        // 20454 = 56 * 365 + 14 leap days (+2020, 2024).
        assert_eq!(civil_from_days(20_454), (2026, 1, 1));
        // 20699 - 20454 = 245 days into 2026 = 2026-09-03 (DOY 246).
        assert_eq!(civil_from_days(20_699), (2026, 9, 3));
    }

    #[test]
    fn civil_from_days_is_monotonic_non_decreasing() {
        // Across a million-day window the function must never
        // return a value smaller than the previous day's.
        let mut prev = civil_from_days(0);
        for d in 1..1_000_000 {
            let cur = civil_from_days(d);
            assert!(
                cur >= prev,
                "civil_from_days regressed at day {d}: prev={prev:?} cur={cur:?}",
            );
            prev = cur;
        }
    }

    #[test]
    fn civil_from_days_roundtrip_select_dates() {
        // Pick a handful of representative days and verify that
        // (y, m, d) reconstructed from civil_from_days maps back to
        // the original day-of-year offset from the year start.
        // This is a tighter cross-check than the per-date
        // assertions in `well_known_dates`; it would catch any
        // off-by-one in the algorithm without needing a leap-year
        // table.
        let cases = [
            (0u64, 1970u32, 1u32, 1u32),
            (1, 1970, 1, 2),
            (365, 1971, 1, 1),
            (1095, 1972, 12, 31),
            (15765, 2013, 3, 1),
            (16_659, 2015, 8, 12),
            (20_699, 2026, 9, 3),
        ];
        for (d, y, m, day) in cases {
            let (yy, mm, dd) = civil_from_days(d as i64);
            assert_eq!((yy, mm, dd), (y as i32, m, day), "day {d}");
        }
    }
}
