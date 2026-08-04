# coverage_check

Per-crate line-coverage gate for StellopayCore Soroban contracts.

The GitHub Actions workflow enforces a minimum **95% line coverage** on the
`src/` of every contract crate in `onchain/contracts/`. This tool is the gate:
it reads `cargo llvm-cov --json` output and fails when any checked crate drops
below the threshold.

It is a **pure checker**: it does NOT invoke `cargo llvm-cov` or `cargo test`.
The CI workflow is responsible for building and measuring coverage first. This
separation gives GitHub Actions native error reporting for build/measurement
failures distinct from coverage-gate failures, and keeps the tool fast and easy
to audit.

## Why `cargo llvm-cov`?

`docs/ci.md` already calls out `cargo llvm-cov` as the intended coverage tool.
It supports `--json`, is Rust-native, and works with `cargo`'s test harness
without custom instrumentation.

## Usage

CI invocation (per crate, inside the `onchain/` workspace):

```bash
cargo llvm-cov -p <package> --json --output-path /tmp/<package>.json
python3 tools/coverage_check/check_coverage.py \
  --report /tmp/<package>.json \
  --workspace onchain \
  --crate <package>
```

Aggregate gate (fail if any crate was silently skipped by the matrix):

```bash
python3 tools/coverage_check/check_coverage.py \
  --report /tmp/coverage-reports/ \
  --workspace onchain
```

Local run on the whole workspace:

```bash
cd onchain
cargo llvm-cov --workspace --json --output-path /tmp/coverage.json
cd ..
python3 tools/coverage_check/check_coverage.py \
  --report /tmp/coverage.json --workspace onchain
```

### Flags

| Flag | Default | Description |
|------|---------|-------------|
| `--report <path>`      | _required_ | A single `cargo llvm-cov --json` file, or a directory of such files (glob `*.json`) |
| `--workspace <path>`   | `onchain` | Cargo workspace root **containing** the `contracts/` directory |
| `--min-line-pct <n>`   | `95`        | Minimum line-coverage percentage per crate |
| `--crate <package>`    | all         | Evaluate only this one package (used by per-crate matrix jobs) |

### Coverage accounting

- Only files under `contracts/<dir>/src/` count toward a crate's coverage.
  Tests drive the measurement but are not the metric.
- Package names are discovered from each `contracts/<dir>/Cargo.toml` `name`
  and matched against report file paths via the directory + `/src/` marker, so
  prefix packages (`rbac` vs `rbac-interface`) never collide.
- If the same file appears more than once across reports, the entry with the
  largest measured total is kept (dedupes re-built artifacts in one aggregate).
- A crate is measured at exactly `95.0%` or above → PASS; anything below, or
  with **no measurable lines**, or **absent from the reports entirely** → FAIL.
  The aggregate mode requires every discovered crate to be present, so a crate
  accidentally omitted from the workflow matrix cannot pass silently.

Exit code is `0` if every checked crate passes. `1` if any crate is below the
threshold, has no coverage data, or is absent from the reports. `2` is a usage
error (unknown `--crate`, invalid threshold, missing report path). A Markdown
summary is printed to stdout and, when `GITHUB_STEP_SUMMARY` is set in the
environment, appended to that file for the GitHub Actions job summary.

## Tests

```bash
python3 -m unittest discover -s tools/coverage_check/tests -v
```

Coverage targets the report parsing, crate discovery and prefix-safe matching,
aggregation (dedupe, absolute/relative/Windows paths), threshold evaluation,
and summary rendering, plus end-to-end `main()` flows (pass, fail, missing
crate, step-summary output, exit codes).

## Why Python and not Rust?

The other checkers in this repo (`tools/wasm_size_check`) are Rust because they
run from inside a `cargo` project. The coverage gate consumes JSON and only
filters/aggregates/evaluates, so a small dependency-free Python script keeps the
audit surface tiny while exercising `GITHUB_STEP_SUMMARY` for the per-crate
summary the gate requires.
