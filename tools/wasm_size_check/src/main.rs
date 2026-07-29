//! CLI entry point for `wasm_size_check`.
//!
//! The binary is intentionally thin: all logic lives in the
//! [`wasm_size_check`](crate) library so it can be exercised by
//! integration tests without going through `clap` and the process
//! boundary.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use wasm_size_check::{compare, refresh, Baseline, Error};

/// `wasm_size_check` — Soroban contract WASM-binary-size regression
/// checker for `.github/workflows/contracts.yml`.
#[derive(Debug, Parser)]
#[command(
    name = "wasm_size_check",
    about = "Compare compiled Soroban WASM artifacts against a committed JSON size baseline",
    long_about = None,
    version
)]
struct Cli {
    /// Path to the committed baseline JSON file (e.g.
    /// `benchmarks/wasm_sizes.json`). On first bootstrap this file
    /// may be missing — pass `--update-baseline` to create it.
    #[arg(long, value_name = "PATH")]
    baseline: PathBuf,

    /// Directory produced by
    /// `cargo build --release --target wasm32-unknown-unknown`
    /// (typically `onchain/target/wasm32-unknown-unknown/release`).
    #[arg(long, value_name = "DIR")]
    wasm_dir: PathBuf,

    /// Maximum allowed percent growth before a contract is reported
    /// as regressing. Default 5%.
    #[arg(long, default_value_t = 5.0, value_name = "PCT")]
    tolerance_pct: f64,

    /// Refresh the baseline with the current measurements and exit 0.
    /// The refreshed baseline is written back to `--baseline`.
    #[arg(long, default_value_t = false)]
    update_baseline: bool,

    /// With the default compare-mode, fail when a `.wasm` is present
    /// that has no entry in the baseline (a new contract that hasn't
    /// recorded a baseline value yet).
    #[arg(long, default_value_t = false)]
    fail_on_new: bool,

    /// In compare-mode, do not fail when a baseline entry has no
    /// matching `.wasm` on disk. Useful for temporarily-disabled
    /// contracts.
    #[arg(long, default_value_t = false)]
    allow_missing: bool,

    /// Also write the Markdown report to this file (in addition to
    /// stdout).
    #[arg(long, value_name = "PATH")]
    report: Option<PathBuf>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("wasm_size_check: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli) -> Result<(), Error> {
    let baseline = load_baseline(&cli.baseline, cli.update_baseline)?;

    if cli.update_baseline {
        let updated = refresh(&baseline, &cli.wasm_dir, None, Some(&cli.baseline))?;
        println!(
            "wasm_size_check: baseline refreshed — {} contract(s) recorded in {}",
            updated.contracts.len(),
            cli.baseline.display(),
        );
        for (name, entry) in &updated.contracts {
            println!(
                "  · {}  {} bytes  {}",
                name,
                entry.size_bytes,
                entry.sha256.as_deref().unwrap_or("?"),
            );
        }
        return Ok(());
    }

    let report = compare(
        &baseline,
        &cli.wasm_dir,
        cli.tolerance_pct,
        cli.fail_on_new,
        !cli.allow_missing,
    )?;
    let md = report.to_markdown();
    println!("{md}");
    if let Some(path) = &cli.report {
        std::fs::write(path, &md).map_err(|source| Error::BaselineWrite {
            path: path.clone(),
            source,
        })?;
    }
    if report.has_failures() {
        eprintln!(
            "wasm_size_check: FAIL (tolerance {}%, {} pass / {} fail — see report above)",
            cli.tolerance_pct,
            count_pass(&report),
            count_fail(&report),
        );
        std::process::exit(1);
    }
    println!(
        "wasm_size_check: OK (tolerance {}%, {} contract(s) checked)",
        cli.tolerance_pct,
        report.comparisons.len()
    );
    Ok(())
}

fn load_baseline(path: &PathBuf, update: bool) -> Result<Baseline, Error> {
    if path.exists() {
        return Baseline::read(path);
    }
    if update {
        return Ok(Baseline::default());
    }
    Err(Error::BaselineMissingForCompare { path: path.clone() })
}

fn count_pass(report: &wasm_size_check::Report) -> usize {
    report
        .comparisons
        .iter()
        .filter(|c| !c.status.is_failure())
        .count()
}

fn count_fail(report: &wasm_size_check::Report) -> usize {
    // The MissingWasm rows are already in `report.comparisons`, and
    // `Report::remaining_baseline_only` is derived from them, so we
    // must not add `remaining_baseline_only.len()` here — it would
    // double-count every missing-artifact failure in the summary line.
    report
        .comparisons
        .iter()
        .filter(|c| c.status.is_failure())
        .count()
}

