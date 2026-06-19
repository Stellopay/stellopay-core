use std::env;
use std::process;

use doc_checker::{scan_contracts_dir, CliAction, Config};

fn main() {
    let config = match Config::from_args(env::args().skip(1)) {
        Ok(CliAction::Run(config)) => config,
        Ok(CliAction::Help) => {
            println!("{}", Config::usage());
            return;
        }
        Err(error) => {
            eprintln!("{error}\n\n{}", Config::usage());
            process::exit(2);
        }
    };

    let findings = match scan_contracts_dir(&config.contracts_dir) {
        Ok(findings) => findings,
        Err(error) => {
            eprintln!("failed to scan {}: {error}", config.contracts_dir.display());
            process::exit(2);
        }
    };

    for finding in &findings {
        println!("{}: {finding}", config.severity.label());
    }
    println!("Total documentation findings: {}", findings.len());

    if config.severity == doc_checker::Severity::Fail && !findings.is_empty() {
        process::exit(1);
    }
}
