use doc_checker::baseline::{partition_findings, BaselineFile};
use doc_checker::{check_docs, check_orphaned_docs, check_stale_doc_references, CheckConfig, Severity};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_help();
        return;
    }

    let mut config = CheckConfig::default();
    config.check_events = args.iter().any(|arg| arg == "--events" || arg == "-e");
    if args.iter().any(|arg| arg == "--strict") {
        config.new_check_severity = Severity::Fail;
    }
    if args.iter().any(|arg| arg == "--no-undocumented-fns") {
        config.check_undocumented_fns = false;
    }
    if args.iter().any(|arg| arg == "--no-error-enums") {
        config.check_error_enums = false;
    }
    if args.iter().any(|arg| arg == "--no-orphaned-docs") {
        config.check_orphaned_docs = false;
    }
    if args.iter().any(|arg| arg == "--no-stale-refs") {
        config.check_stale_refs = false;
    }

    // Optional committed baseline of pre-existing violations. Findings whose
    // identity appears in the baseline are downgraded to warnings; findings
    // with no baseline entry fail under --strict. See src/baseline.rs.
    let update_baseline = args.iter().any(|arg| arg == "--update-baseline");
    let baseline_path: Option<PathBuf> = args
        .iter()
        .position(|arg| arg == "--baseline")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from);

    let mut findings = collect_findings(&config);

    if update_baseline {
        let path = baseline_path.unwrap_or_else(doc_checker::baseline::default_baseline_path);
        let (baseline, unclassified) = BaselineFile::regenerate(&findings);
        for finding in &unclassified {
            println!("warning: could not classify, NOT added to baseline: {}", finding.message);
        }
        baseline.save(&path).unwrap_or_else(|e| {
            eprintln!("error: failed to write baseline {}: {}", path.display(), e);
            std::process::exit(1);
        });
        println!(
            "Baseline written to {} ({} entrie(s)); unclassified findings: {}",
            path.display(),
            baseline.violations.len(),
            unclassified.len()
        );
        return;
    }

    let baseline = match &baseline_path {
        Some(path) => match BaselineFile::load(path) {
            Ok(loaded) => loaded,
            Err(e) => {
                // A broken baseline must fail the run, never silently
                // disable enforcement.
                eprintln!("error: {}", e);
                std::process::exit(1);
            }
        },
        None => None,
    };
    let partition = partition_findings(&findings, baseline.as_ref());
    findings.clear();

    let mut warnings = 0usize;
    let mut failures = 0usize;

    for finding in partition.baselined.iter().chain(partition.unmatched.iter()) {
        report(finding, &mut warnings, &mut failures);
    }
    for entry in &partition.stale_entries {
        // Pre-existing violations that were fixed leave a stale baseline
        // entry behind. This is only a warning: shrinking the baseline is
        // good, but removing entries is an explicit, reviewable change so
        // the reviewer sees the backlog actually shrank.
        println!("warning: stale baseline entry (violation appears fixed): {}", entry);
        warnings += 1;
    }

    println!(
        "Documentation findings: {} error(s), {} warning(s)",
        failures, warnings
    );
    if failures > 0 {
        std::process::exit(1);
    }
}

/// Gathers findings from all check categories, in a stable order
/// (contract files in sorted walk order, then orphaned docs, then stale
/// references) so output and baseline generation are deterministic.
fn collect_findings(config: &CheckConfig) -> Vec<doc_checker::Finding> {
    let mut findings = Vec::new();

    let mut rs_files: Vec<PathBuf> = WalkDir::new("../../onchain/contracts")
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().extension().map_or(false, |ext| ext == "rs"))
        .map(|entry| entry.path().to_path_buf())
        .collect();
    rs_files.sort();

    for path in rs_files {
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let file_name = path.display().to_string();
        findings.extend(check_docs(&content, &file_name, config));
    }

    let repo_root = Path::new("../..");
    findings.extend(check_orphaned_docs(repo_root, config));
    findings.extend(check_stale_doc_references(repo_root, config));

    findings
}

fn report(finding: &doc_checker::Finding, warnings: &mut usize, failures: &mut usize) {
    let label = match finding.severity {
        Severity::Warn => "warning",
        Severity::Fail => "error",
    };
    println!("{}: {}", label, finding.message);
    match finding.severity {
        Severity::Warn => *warnings += 1,
        Severity::Fail => *failures += 1,
    }
}

fn print_help() {
    println!(
        "Usage: cargo run [--release] -- [OPTIONS]

Documentation rules for the Soroban contract tree.

Options:
  --events, -e             Also check event-like #[contracttype] items.
  --strict                 Promote newer checks from warnings to errors.
  --baseline <PATH>        Enforce against the committed baseline at PATH:
                           baselined findings warn, new findings fail.
  --update-baseline        Recompute the baseline (with --baseline <PATH>
                           if given, else tools/doc_checker/baseline.json)
                           from the current findings and write it out.
  --no-undocumented-fns    Skip the undocumented-fn check.
  --no-error-enums         Skip the #[contracterror] variant check.
  --no-orphaned-docs       Skip the orphaned docs/*.md check.
  --no-stale-refs          Skip the stale doc reference check.
  -h, --help               Show this help."
    );
}
