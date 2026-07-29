//! Comparison + Markdown report types.
//!
//! The checker walks the union of baseline keys and measured
//! contracts, classifies each pair, and the [`Report`] renders the
//! outcome as both a structured (testable) value and a Markdown
//! summary suitable for GitHub step summaries.

use std::collections::BTreeMap;

use crate::baseline::{Baseline, BaselineEntry};
use crate::inventory::Measurement;

/// Classification of a single contract's baseline-vs-current
/// comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ComparisonStatus {
    /// Compared against an existing baseline entry that matches the
    /// recorded size and (if present) hash within the configured
    /// tolerance.
    Ok,

    /// The contract shrank relative to the baseline. Always a pass
    /// but reported so reviewers can spot unexpectedly removed code.
    Shrunk,

    /// A `.wasm` was found with no matching baseline entry. With
    /// `--fail-on-new` (default off) this is informational; otherwise
    /// it's a hard failure.
    NewContract,

    /// The baseline entry's recorded SHA-256 does not match the
    /// artifact's hash at the same size. Almost always means the
    /// baseline entry was copy/pasted from a different run — treated
    /// as a hard failure so it can't slip through review.
    HashMismatch,

    /// The contract's compiled size grew beyond the configured
    /// tolerance. Hard failure.
    Regression,

    /// The baseline has an entry for this contract but no
    /// corresponding `.wasm` artifact on disk. Treated as a soft
    /// failure (status only) when `--allow-missing` is passed,
    /// otherwise a hard failure.
    MissingWasm,
}

impl ComparisonStatus {
    /// Short, stable label used both in Markdown rows and terminal
    /// output. The labels are part of the tool's user-facing contract;
    /// changing them is a breaking change for log scrapers.
    pub fn label(self) -> &'static str {
        match self {
            ComparisonStatus::Ok => "ok",
            ComparisonStatus::Shrunk => "shrunk",
            ComparisonStatus::NewContract => "new",
            ComparisonStatus::HashMismatch => "hash mismatch",
            ComparisonStatus::Regression => "REGRESSION",
            ComparisonStatus::MissingWasm => "missing",
        }
    }

    /// `true` if this comparison should fail CI.
    pub fn is_failure(self) -> bool {
        matches!(
            self,
            ComparisonStatus::NewContract
                | ComparisonStatus::HashMismatch
                | ComparisonStatus::Regression
                | ComparisonStatus::MissingWasm
        )
    }
}

/// One row in the [`Report`] table.
#[derive(Debug, Clone)]
pub struct Comparison {
    /// Contract name (`.wasm` file stem).
    pub name: String,
    /// Baseline entry, if any.
    pub baseline: Option<BaselineEntry>,
    /// Measurement taken from disk, if any.
    pub measurement: Option<Measurement>,
    /// Outcome classification.
    pub status: ComparisonStatus,
    /// Byte difference `measurement - baseline`. Negative when the
    /// contract shrank, positive when it grew, zero when unchanged.
    /// For new contracts this is the full size. For missing WASM
    /// files this is the negative of the baseline size.
    pub delta_bytes: i64,
    /// Percent difference relative to the baseline. `0.0` for new
    /// contracts, `f64::INFINITY` for "grew from a zero-byte baseline"
    /// (should never happen in practice for a real Soroban contract).
    pub delta_pct: f64,
}

/// Aggregated report for one compare pass.
#[derive(Debug, Clone)]
pub struct Report {
    /// Tolerance (percent) used to classify regressions.
    pub tolerance_pct: f64,
    /// Per-contract comparisons, sorted by name.
    pub comparisons: Vec<Comparison>,
    /// Names of baseline entries that have no corresponding `.wasm`
    /// on disk and that are not being skipped via `--allow-missing`.
    /// Carried as a separate field for the Markdown "missing entries"
    /// section.
    pub remaining_baseline_only: Vec<String>,
}

impl Report {
    /// `true` when any comparison failed or any baseline-only entry
    /// is missing on disk.
    pub fn has_failures(&self) -> bool {
        self.comparisons.iter().any(|c| c.status.is_failure())
            || !self.remaining_baseline_only.is_empty()
    }

    /// Count occurrences of each status for the Markdown summary header.
    pub fn status_counts(&self) -> BTreeMap<&'static str, usize> {
        let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
        for c in &self.comparisons {
            *counts.entry(c.status.label()).or_insert(0) += 1;
        }
        counts
    }

    /// Render the report as a Markdown table. The output is suitable
    /// both for human inspection and for piping to `gh-actions` step
    /// summaries (written to `$GITHUB_STEP_SUMMARY`).
    ///
    /// Implementation note: this method intentionally avoids the
    /// `writeln!` macro family and writes string fragments directly.
    /// This keeps output byte-stable across toolchain versions and
    /// removes a class of macro-parsing edge cases when the tool is
    /// invoked from unusual shell contexts.
    pub fn to_markdown(&self) -> String {
        let mut s = String::new();
        s.push_str("# WASM size regression report\n\n");
        s.push_str(&format!(
            "_tolerance: {}% · contracts checked: {}{}_\n\n",
            self.tolerance_pct,
            self.comparisons.len(),
            if self.remaining_baseline_only.is_empty() {
                String::new()
            } else {
                " · missing artifacts: see below".to_string()
            },
        ));

        let mut counts = self.status_counts();
        // `passes` are non-failing statuses (ok + shrunk).
        let passes = counts.remove("ok").unwrap_or(0) + counts.remove("shrunk").unwrap_or(0);
        // `failures` are statuses whose `is_failure()` is `true`
        // (NewContract, HashMismatch, Regression, MissingWasm — all of
        // which are already counted via `status_counts()` on
        // `self.comparisons`). Do NOT add `remaining_baseline_only.len()`
        // here: that field is derived from the same MissingWasm rows
        // and adding it would double-count every missing-artifact
        // failure in the displayed summary.
        let failures = counts.values().sum::<usize>();
        s.push_str(&format!(
            "_summary: {} pass / {} fail_\n\n",
            passes, failures
        ));

        s.push_str("| Contract | Baseline | Measured | Δ bytes | Δ % | Status |\n");
        s.push_str("|---|---:|---:|---:|---:|---|\n");
        for c in &self.comparisons {
            let baseline = c
                .baseline
                .as_ref()
                .map(|e| e.size_bytes.to_string())
                .unwrap_or_else(|| "—".to_string());
            let measured = c
                .measurement
                .as_ref()
                .map(|m| m.size_bytes.to_string())
                .unwrap_or_else(|| "—".to_string());
            let delta_bytes = format!("{:+}", c.delta_bytes);
            let delta_pct = if c.delta_pct.is_finite() {
                format!("{:+.3}", c.delta_pct)
            } else {
                "∞".to_string()
            };
            s.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} |\n",
                c.name,
                baseline,
                measured,
                delta_bytes,
                delta_pct,
                c.status.label(),
            ));
        }

        if !self.remaining_baseline_only.is_empty() {
            s.push_str("\n## Missing artifacts\n\n");
            s.push_str("Baseline entries without a corresponding `.wasm` on disk:\n\n");
            for name in &self.remaining_baseline_only {
                s.push_str(&format!("- {}\n", name));
            }
        }

        s
    }
}

/// Build a [`Report`] from a baseline + measurement map.
///
/// This is the internal helper called by [`crate::command::compare`]
/// after arg parsing. Kept in this module so unit tests can exercise
/// the classification logic directly without touching disk.
///
/// `fail_on_new` and `fail_on_missing` mirror the CLI flags of the
/// same name (negated for `--allow-missing`).
pub fn classify(
    baseline: &Baseline,
    measurements: &BTreeMap<String, Measurement>,
    tolerance_pct: f64,
    fail_on_new: bool,
    fail_on_missing: bool,
) -> Report {
    let mut comparisons: Vec<Comparison> = Vec::with_capacity(
        baseline.contracts.len() + measurements.len(),
    );

    // Measured (on-disk) contracts.
    for (name, m) in measurements {
        let baseline_entry = baseline.contracts.get(name).cloned();
        let (status, delta_bytes, delta_pct) = classify_one(
            baseline_entry.as_ref(),
            Some(m),
            tolerance_pct,
            fail_on_new,
        );
        comparisons.push(Comparison {
            name: name.clone(),
            baseline: baseline_entry,
            measurement: Some(m.clone()),
            status,
            delta_bytes,
            delta_pct,
        });
    }

    // Baseline-only entries (no matching `.wasm` on disk).
    for (name, entry) in &baseline.contracts {
        if measurements.contains_key(name) {
            continue;
        }
        let status = if fail_on_missing {
            ComparisonStatus::MissingWasm
        } else {
            ComparisonStatus::Ok
        };
        comparisons.push(Comparison {
            name: name.clone(),
            baseline: Some(entry.clone()),
            measurement: None,
            status,
            delta_bytes: -(entry.size_bytes as i64),
            delta_pct: -100.0,
        });
    }

    comparisons.sort_by(|a, b| a.name.cmp(&b.name));

    // `remaining_baseline_only` lists every baseline entry whose
    // matching `.wasm` is absent on disk AND the operator has chosen
    // to treat as a failure. When `--allow-missing` is passed we
    // still record the entry in `comparisons` (status `Ok`) so the
    // table reports it, but we exclude it from the "missing"
    // subsection so reviewers don't see a failure list they already
    // opted to suppress.
    let remaining_baseline_only: Vec<String> = comparisons
        .iter()
        .filter(|c| c.status == ComparisonStatus::MissingWasm)
        .map(|c| c.name.clone())
        .collect();

    Report {
        tolerance_pct,
        comparisons,
        remaining_baseline_only,
    }
}

fn classify_one(
    baseline: Option<&BaselineEntry>,
    measurement: Option<&Measurement>,
    tolerance_pct: f64,
    fail_on_new: bool,
) -> (ComparisonStatus, i64, f64) {
    match (baseline, measurement) {
        (None, Some(m)) => {
            let status = if fail_on_new {
                ComparisonStatus::NewContract
            } else {
                ComparisonStatus::Ok
            };
            (status, m.size_bytes as i64, 0.0)
        }
        (Some(b), Some(m)) => {
            let delta_bytes = m.size_bytes as i64 - b.size_bytes as i64;
            let delta_pct = if b.size_bytes == 0 {
                if m.size_bytes == 0 {
                    0.0
                } else {
                    f64::INFINITY
                }
            } else {
                (delta_bytes as f64) * 100.0 / (b.size_bytes as f64)
            };
            // Hash drift is only meaningful when the compiled size is
            // unchanged — different sizes necessarily produce different
            // bytes (and therefore different hashes). Flagging
            // HashMismatch on every size change would create spurious
            // failures on every legitimate size bump, defeating the
            // signal: we reserve HashMismatch for the case where the
            // recorded baseline entry was copy-pasted from a different
            // run at the same size.
            let size_matches = b.size_bytes == m.size_bytes;
            let hashes_match = match &b.sha256 {
                Some(recorded) => recorded == &m.sha256,
                None => true, // No recorded hash → nothing to verify.
            };
            let hash_drift = size_matches && !hashes_match;
            let grew_beyond_tolerance =
                delta_pct > tolerance_pct && m.size_bytes > b.size_bytes;
            let status = if hash_drift {
                ComparisonStatus::HashMismatch
            } else if grew_beyond_tolerance {
                ComparisonStatus::Regression
            } else if delta_bytes < 0 {
                ComparisonStatus::Shrunk
            } else {
                ComparisonStatus::Ok
            };
            (status, delta_bytes, delta_pct)
        }
        (Some(b), None) => (
            ComparisonStatus::MissingWasm,
            -(b.size_bytes as i64),
            -100.0,
        ),
        (None, None) => {
            unreachable!("classify_one called with neither baseline nor measurement")
        }
    }
}
