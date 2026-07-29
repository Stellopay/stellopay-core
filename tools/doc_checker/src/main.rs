use doc_checker::{check_docs, check_orphaned_docs, check_stale_doc_references, CheckConfig, Severity};
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

fn main() {
    let args: Vec<String> = std::env::args().collect();
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

    let mut warnings = 0;
    let mut failures = 0;

    for entry in WalkDir::new("../../onchain/contracts") {
        let entry = entry.unwrap();
        if entry.path().extension().map_or(false, |ext| ext == "rs") {
            let content = fs::read_to_string(entry.path()).unwrap();
            let file_name = entry.path().display().to_string();
            let findings = check_docs(&content, &file_name, &config);
            for finding in &findings {
                report(finding, &mut warnings, &mut failures);
            }
        }
    }

    let repo_root = Path::new("../..");
    for finding in check_orphaned_docs(repo_root, &config) {
        report(&finding, &mut warnings, &mut failures);
    }

    for finding in check_stale_doc_references(repo_root, &config) {
        report(&finding, &mut warnings, &mut failures);
    }

    println!(
        "Documentation findings: {} error(s), {} warning(s)",
        failures, warnings
    );
    if failures > 0 {
        std::process::exit(1);
    }
}

fn report(finding: &doc_checker::Finding, warnings: &mut u32, failures: &mut u32) {
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
