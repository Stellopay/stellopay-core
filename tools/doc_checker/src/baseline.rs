//! Committed baseline of pre-existing documentation violations.
//!
//! The documentation checks were introduced incrementally (first as
//! warnings, later promoted via `--strict`), so a backlog of pre-existing
//! violations exists. This module implements the same pattern the WASM
//! size checker uses: a committed, reviewed JSON baseline records the
//! violations that existed when enforcement was switched on.
//!
//! # Semantics
//!
//! * A finding whose *identity* appears in the baseline is downgraded to a
//!   warning — it is still printed, but does not fail the run.
//! * A finding with no baseline entry fails the run (under `--strict`), so
//!   **the baseline cannot grow silently**: every new violation has a new
//!   identity and fails CI.
//! * Baseline entries that no longer match any current finding are
//!   reported as stale; they can only be removed by regenerating the
//!   baseline (an explicit, reviewable diff).
//!
//! # Fail-closed design
//!
//! Identities are derived by parsing the finding messages this crate
//! itself emits. A message that cannot be parsed into a stable identity is
//! **never** recorded into a baseline and never matches one, so unknown
//! finding formats (e.g. from a future rule) always fail — the baseline can
//! never accidentally swallow a finding type it was not built for.
//!
//! # Identity stability
//!
//! Identities deliberately exclude line numbers (fixing an unrelated
//! violation above a baselined one must not re-fail CI) and normalize path
//! separators and `tools/doc_checker`-relative prefixes, so the baseline is
//! diff-stable and OS-independent.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::{Finding, Severity};

/// Current schema version of the on-disk baseline format. Bumping this is a
/// breaking change and must be coordinated with a concurrent
/// `--update-baseline` run.
pub const BASELINE_VERSION: u32 = 1;

/// Default baseline location: next to the crate manifest
/// (`tools/doc_checker/baseline.json`).
pub fn default_baseline_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("baseline.json")
}

/// The on-disk baseline document.
///
/// ```json
/// {
///   "version": 1,
///   "updated": "2026-09-25",
///   "violations": [
///     "onchain/contracts/multisig/src/lib.rs|fn-undocumented|deposit"
///   ]
/// }
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaselineFile {
    /// Schema version. Always [`BASELINE_VERSION`] on write; validated on load.
    #[serde(default = "default_version")]
    pub version: u32,

    /// ISO date (`YYYY-MM-DD`, UTC) the baseline was last regenerated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated: Option<String>,

    /// Set of baselined finding identities. A `BTreeSet` so the JSON is
    /// deterministically ordered and diffs cleanly in code review.
    #[serde(default)]
    pub violations: BTreeSet<String>,
}

fn default_version() -> u32 {
    BASELINE_VERSION
}

impl BaselineFile {
    /// Loads the baseline at `path`. Returns `Ok(None)` when the file does
    /// not exist (no baseline in effect), and an error for unreadable or
    /// schema-incompatible files.
    pub fn load(path: &Path) -> io::Result<Option<BaselineFile>> {
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(path)?;
        let baseline: BaselineFile = serde_json::from_str(&content).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{} is not a valid doc_checker baseline: {}", path.display(), e),
            )
        })?;
        if baseline.version != BASELINE_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "{} has baseline version {} but this build understands version {}",
                    path.display(),
                    baseline.version,
                    BASELINE_VERSION
                ),
            ));
        }
        Ok(Some(baseline))
    }

    /// Writes the baseline as pretty-printed, deterministically ordered JSON.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, json + "\n")
    }

    /// Builds a fresh baseline capturing every classifiable finding in
    /// `findings`. Findings whose message cannot be classified are returned
    /// in `unclassified` and are deliberately **not** recorded (fail-closed:
    /// they will keep failing the run until understood).
    pub fn regenerate(findings: &[Finding]) -> (BaselineFile, Vec<&Finding>) {
        let mut violations = BTreeSet::new();
        let mut unclassified = Vec::new();
        for finding in findings {
            match finding_identity(&finding.message) {
                Some(identity) => {
                    violations.insert(identity);
                }
                None => unclassified.push(finding),
            }
        }
        (
            BaselineFile {
                version: BASELINE_VERSION,
                updated: Some(iso_date_today()),
                violations,
            },
            unclassified,
        )
    }
}

/// Result of splitting findings against a baseline.
#[derive(Debug, Default)]
pub struct Partitioned {
    /// Findings present in the baseline; always downgraded to
    /// [`Severity::Warn`] by the caller.
    pub baselined: Vec<Finding>,
    /// Findings with no baseline entry; keep their configured severity and
    /// fail the run when that severity is [`Severity::Fail`].
    pub unmatched: Vec<Finding>,
    /// Baseline identities that matched no current finding. Reported as
    /// stale; removal requires regenerating the baseline.
    pub stale_entries: Vec<String>,
}

/// Splits `findings` into baselined vs unmatched, and reports baseline
/// entries that no longer match anything as stale. With `baseline == None`
/// every finding is unmatched (no baseline in effect).
pub fn partition_findings(findings: &[Finding], baseline: Option<&BaselineFile>) -> Partitioned {
    let mut result = Partitioned::default();
    let Some(baseline) = baseline else {
        result.unmatched = findings.to_vec();
        return result;
    };

    let mut matched = BTreeSet::new();
    for finding in findings {
        match finding_identity(&finding.message) {
            Some(identity) if baseline.violations.contains(&identity) => {
                matched.insert(identity);
                result.baselined.push(Finding {
                    severity: Severity::Warn,
                    message: finding.message.clone(),
                });
            }
            _ => result.unmatched.push(finding.clone()),
        }
    }

    result.stale_entries = baseline
        .violations
        .iter()
        .filter(|entry| !matched.contains(*entry))
        .cloned()
        .collect();
    result
}

/// Normalizes the file-path prefix of a finding message to a stable,
/// repo-root-relative form: forward slashes, no leading `../` segments
/// (the binary runs with its working directory at `tools/doc_checker`, so
/// contract paths arrive as `../../onchain/...`).
fn normalize_file_part(raw: &str) -> String {
    let mut path = raw.replace('\\', "/");
    while let Some(stripped) = path.strip_prefix("../") {
        path = stripped.to_string();
    }
    path
}

/// Classifies a finding message into a stable identity
/// (`<file>|<rule>|<item>`) that can be compared against the baseline.
///
/// Returns `None` for unrecognized messages — those findings can never be
/// baselined and always fail (fail-closed).
pub fn finding_identity(message: &str) -> Option<String> {
    let rules = classification_rules();
    for (rule, key) in rules.iter() {
        if let Some(caps) = rule.captures(message) {
            let file = normalize_file_part(caps.name("file").unwrap().as_str());
            let item = match *key {
                // `fn NAME missing params, return, access control`
                // `fn NAME has no doc comment at all`
                "fn-missing-sections" | "fn-undocumented" | "stale-ref" => caps
                    .name("item_a")
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default(),
                // `struct S missing docs for field F` / `... for unnamed field I`
                // `enum E missing docs for variant V`
                // `error enum E variant V has no doc comment`
                "event-field" | "event-unnamed-field" | "event-variant" | "error-variant" => format!(
                    "{}::{}",
                    caps.name("item_a").map(|m| m.as_str()).unwrap_or(""),
                    caps.name("item_b").map(|m| m.as_str()).unwrap_or("")
                ),
                // `docs/x.md: orphaned doc - ...` — the file itself is the item.
                "orphaned-doc" => file.clone(),
                _ => return None,
            };
            return Some(format!("{}|{}|{}", file, key, item));
        }
    }
    None
}

/// The ordered set of (regex, rule key) pairs used by
/// [`finding_identity`]. Order matters only where messages could overlap;
/// each pattern here is mutually exclusive with the others.
fn classification_rules() -> &'static Vec<(Regex, &'static str)> {
    static RULES: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    RULES.get_or_init(|| {
        let rule = |pattern: &str, key: &'static str| (Regex::new(pattern).unwrap(), key);
        vec![
            // `{file}:{line}: fn {name} missing {parts}`
            rule(
                r"^(?P<file>.+):(?P<line>\d+): fn (?P<item_a>\w+) missing .+$",
                "fn-missing-sections",
            ),
            // `{file}:{line}: fn {name} has no doc comment at all`
            rule(
                r"^(?P<file>.+):(?P<line>\d+): fn (?P<item_a>\w+) has no doc comment at all$",
                "fn-undocumented",
            ),
            // `{file}:{line}: struct {S} missing docs for field {f}`
            rule(
                r"^(?P<file>.+):(?P<line>\d+): struct (?P<item_a>\w+) missing docs for field (?P<item_b>\w+)$",
                "event-field",
            ),
            // `{file}: struct {S} missing docs for unnamed field {i}` (no line number)
            rule(
                r"^(?P<file>.+): struct (?P<item_a>\w+) missing docs for unnamed field (?P<item_b>\d+)$",
                "event-unnamed-field",
            ),
            // `{file}:{line}: enum {E} missing docs for variant {V}`
            rule(
                r"^(?P<file>.+):(?P<line>\d+): enum (?P<item_a>\w+) missing docs for variant (?P<item_b>\w+)$",
                "event-variant",
            ),
            // `{file}:{line}: error enum {E} variant {V} has no doc comment`
            rule(
                r"^(?P<file>.+):(?P<line>\d+): error enum (?P<item_a>\w+) variant (?P<item_b>\w+) has no doc comment$",
                "error-variant",
            ),
            // `{file}: orphaned doc - not reachable from README.md or any docs index file`
            rule(r"^(?P<file>.+): orphaned doc - .*$", "orphaned-doc"),
            // `{file}:{line}: stale doc reference to `{fn}` – no matching pub fn found in contracts`
            rule(
                r"^(?P<file>.+):(?P<line>\d+): stale doc reference to `(?P<item_a>\w+)` .*$",
                "stale-ref",
            ),
        ]
    })
}

/// Today's UTC date as `YYYY-MM-DD`, derived from the system clock without
/// pulling in a date library.
fn iso_date_today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    // Howard Hinnant's civil_from_days algorithm.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    format!("{:04}-{:02}-{:02}", year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finding(msg: &str) -> Finding {
        Finding {
            severity: Severity::Fail,
            message: msg.to_string(),
        }
    }

    // ------------------------------------------------------------------
    // Identity classification: every message format the checker emits
    // ------------------------------------------------------------------

    #[test]
    fn test_identity_undocumented_fn() {
        let id = finding_identity(
            "../../onchain/contracts/multisig/src/lib.rs:37: fn deposit has no doc comment at all",
        )
        .unwrap();
        assert_eq!(
            id,
            "onchain/contracts/multisig/src/lib.rs|fn-undocumented|deposit"
        );
    }

    #[test]
    fn test_identity_fn_missing_sections() {
        let id = finding_identity(
            "../../onchain/contracts/rbac/src/lib.rs:12: fn grant_role missing params, return, access control",
        )
        .unwrap();
        assert_eq!(
            id,
            "onchain/contracts/rbac/src/lib.rs|fn-missing-sections|grant_role"
        );
    }

    #[test]
    fn test_identity_error_variant() {
        let id = finding_identity(
            "../../onchain/contracts/stello_pay_contract/src/lib.rs:88: error enum ContractError variant Unauthorized has no doc comment",
        )
        .unwrap();
        assert_eq!(
            id,
            "onchain/contracts/stello_pay_contract/src/lib.rs|error-variant|ContractError::Unauthorized"
        );
    }

    #[test]
    fn test_identity_event_field() {
        let id = finding_identity(
            "onchain/contracts/foo/src/lib.rs:5: struct TransferEvent missing docs for field from",
        )
        .unwrap();
        assert_eq!(
            id,
            "onchain/contracts/foo/src/lib.rs|event-field|TransferEvent::from"
        );
    }

    #[test]
    fn test_identity_event_unnamed_field() {
        let id = finding_identity(
            "onchain/contracts/foo/src/lib.rs: struct TuplePayload missing docs for unnamed field 2",
        )
        .unwrap();
        assert_eq!(
            id,
            "onchain/contracts/foo/src/lib.rs|event-unnamed-field|TuplePayload::2"
        );
    }

    #[test]
    fn test_identity_event_variant() {
        let id = finding_identity(
            "onchain/contracts/foo/src/lib.rs:9: enum OpPayload missing docs for variant B",
        )
        .unwrap();
        assert_eq!(id, "onchain/contracts/foo/src/lib.rs|event-variant|OpPayload::B");
    }

    #[test]
    fn test_identity_orphaned_doc() {
        let id = finding_identity(
            "docs/legacy/old-guide.md: orphaned doc - not reachable from README.md or any docs index file",
        )
        .unwrap();
        assert_eq!(
            id,
            "docs/legacy/old-guide.md|orphaned-doc|docs/legacy/old-guide.md"
        );
    }

    #[test]
    fn test_identity_stale_ref() {
        let id = finding_identity(
            "docs/api.md:41: stale doc reference to `old_func` – no matching pub fn found in contracts",
        )
        .unwrap();
        assert_eq!(id, "docs/api.md|stale-ref|old_func");
    }

    // ------------------------------------------------------------------
    // Stability + fail-closed behavior
    // ------------------------------------------------------------------

    #[test]
    fn test_identity_ignores_line_numbers() {
        let a = finding_identity("docs/a.md:10: fn foo has no doc comment at all").unwrap();
        let b = finding_identity("docs/a.md:9999: fn foo has no doc comment at all").unwrap();
        assert_eq!(a, b, "line numbers must not leak into identities");
    }

    #[test]
    fn test_identity_normalizes_path_separators_and_parent_dirs() {
        let unix = finding_identity("../../onchain/c/src/lib.rs:1: fn foo has no doc comment at all").unwrap();
        let windows =
            finding_identity("..\\..\\onchain\\c\\src\\lib.rs:1: fn foo has no doc comment at all").unwrap();
        assert_eq!(unix, windows);
        assert!(unix.starts_with("onchain/"));
    }

    #[test]
    fn test_unrecognized_message_is_never_classified() {
        // Fail-closed: unknown formats cannot enter or match a baseline.
        assert!(finding_identity("totally unparseable").is_none());
        assert!(finding_identity("").is_none());
        assert!(finding_identity("onchain/x.rs: fn nobody_ever_emits_this").is_none());
    }

    // ------------------------------------------------------------------
    // Partitioning
    // ------------------------------------------------------------------

    #[test]
    fn test_partition_downgrades_baselined_and_flags_unmatched() {
        let baseline = BaselineFile {
            version: BASELINE_VERSION,
            updated: None,
            violations: ["onchain/a.rs|fn-undocumented|baselined_fn".to_string()]
                .into_iter()
                .collect(),
        };
        let findings = vec![
            finding("onchain/a.rs:1: fn baselined_fn has no doc comment at all"),
            finding("onchain/a.rs:2: fn new_violation has no doc comment at all"),
        ];
        let partitioned = partition_findings(&findings, Some(&baseline));
        assert_eq!(partitioned.baselined.len(), 1);
        assert_eq!(partitioned.baselined[0].severity, Severity::Warn);
        assert_eq!(partitioned.unmatched.len(), 1);
        assert_eq!(partitioned.unmatched[0].severity, Severity::Fail);
        assert!(partitioned.stale_entries.is_empty());
    }

    #[test]
    fn test_partition_reports_stale_entries() {
        let baseline = BaselineFile {
            version: BASELINE_VERSION,
            updated: None,
            violations: [
                "onchain/a.rs|fn-undocumented|fixed_fn".to_string(),
                "onchain/a.rs|fn-undocumented|still_broken_fn".to_string(),
            ]
            .into_iter()
            .collect(),
        };
        let findings = vec![finding("onchain/a.rs:1: fn still_broken_fn has no doc comment at all")];
        let partitioned = partition_findings(&findings, Some(&baseline));
        assert_eq!(partitioned.stale_entries, vec![
            "onchain/a.rs|fn-undocumented|fixed_fn".to_string()
        ]);
    }

    #[test]
    fn test_partition_without_baseline_matches_everything_unmatched() {
        let findings = vec![finding("onchain/a.rs:1: fn foo has no doc comment at all")];
        let partitioned = partition_findings(&findings, None);
        assert!(partitioned.baselined.is_empty());
        assert_eq!(partitioned.unmatched.len(), 1);
        assert!(partitioned.stale_entries.is_empty());
    }

    #[test]
    fn test_unclassifiable_findings_cannot_match_a_baseline() {
        // Even if someone hand-edits a baseline with a bogus entry, an
        // unclassifiable finding still comes out unmatched.
        let baseline = BaselineFile {
            version: BASELINE_VERSION,
            updated: None,
            violations: ["weird|hand-edited|entry".to_string()].into_iter().collect(),
        };
        let findings = vec![finding("totally unparseable")];
        let partitioned = partition_findings(&findings, Some(&baseline));
        assert!(partitioned.baselined.is_empty());
        assert_eq!(partitioned.unmatched.len(), 1);
        assert_eq!(
            partitioned.stale_entries,
            vec!["weird|hand-edited|entry".to_string()]
        );
    }

    // ------------------------------------------------------------------
    // Regeneration + persistence
    // ------------------------------------------------------------------

    #[test]
    fn test_regenerate_records_classifiable_and_reports_the_rest() {
        let findings = vec![
            finding("onchain/a.rs:1: fn foo has no doc comment at all"),
            finding("onchain/a.rs:2: fn foo has no doc comment at all"), // duplicate identity
            finding("unparseable garbage"),
        ];
        let (baseline, unclassified) = BaselineFile::regenerate(&findings);
        assert_eq!(baseline.violations.len(), 1, "duplicates must be deduped");
        assert!(baseline.violations.contains("onchain/a.rs|fn-undocumented|foo"));
        assert_eq!(baseline.version, BASELINE_VERSION);
        assert!(baseline.updated.is_some());
        assert_eq!(unclassified.len(), 1);
    }

    fn unique_temp_path(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "doc_checker_baseline_test_{}_{}_{}",
            tag,
            std::process::id(),
            nanos
        ))
    }

    #[test]
    fn test_load_missing_file_is_none() {
        let path = unique_temp_path("missing");
        assert!(BaselineFile::load(&path).unwrap().is_none());
    }

    #[test]
    fn test_save_load_roundtrip_is_deterministic() {
        let path = unique_temp_path("roundtrip");
        let baseline = BaselineFile {
            version: BASELINE_VERSION,
            updated: Some("2026-09-25".to_string()),
            violations: [
                "onchain/b.rs|fn-undocumented|beta".to_string(),
                "onchain/a.rs|fn-undocumented|alpha".to_string(),
            ]
            .into_iter()
            .collect(),
        };
        baseline.save(&path).unwrap();
        let loaded = BaselineFile::load(&path).unwrap().unwrap();
        assert_eq!(loaded, baseline);

        // The on-disk JSON must be deterministically ordered (BTreeSet).
        let text = fs::read_to_string(&path).unwrap();
        let alpha_pos = text.find("onchain/a.rs").unwrap();
        let beta_pos = text.find("onchain/b.rs").unwrap();
        assert!(alpha_pos < beta_pos, "violations must be sorted on disk");

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_load_rejects_wrong_schema_version() {
        let path = unique_temp_path("version");
        fs::write(
            &path,
            r#"{ "version": 999, "violations": [] }"#,
        )
        .unwrap();
        assert!(BaselineFile::load(&path).is_err());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_load_rejects_non_json() {
        let path = unique_temp_path("garbage");
        fs::write(&path, "not json at all").unwrap();
        assert!(BaselineFile::load(&path).is_err());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_iso_date_today_has_expected_shape() {
        let date = iso_date_today();
        assert_eq!(date.len(), 10);
        let bytes = date.as_bytes();
        assert_eq!(bytes[4], b'-');
        assert_eq!(bytes[7], b'-');
        assert!(bytes.iter().enumerate().all(|(i, b)| {
            matches!(i, 4 | 7) || b.is_ascii_digit()
        }));
    }
}
