# wasm_size_check

WASM binary-size regression checker for StellopayCore Soroban contracts.

This tool compares the compiled `.wasm` size of each contract binary against a
committed baseline (JSON) and fails if any contract has grown beyond a
configured percentage tolerance without an explicit baseline update.

It is a **pure checker**: it does NOT invoke `cargo build`. The CI workflow is
responsible for building the contracts to `wasm32-unknown-unknown` first. This
separation gives GitHub Actions native error reporting for build failures
distinct from size regression failures, and keeps the tool fast and easy to
audit.

## Usage

CI invocation (after `cargo build --target wasm32-unknown-unknown --release`):

```bash
cargo run --release -- \
  --baseline ../../benchmarks/wasm_sizes.json \
  --wasm-dir ../../onchain/target/wasm32-unknown-unknown/release \
  --tolerance-pct 5
```

### Flags

| Flag | Default | Description |
|------|---------|-------------|
| `--baseline <path>`    | _required_ | Path to the committed baseline JSON file |
| `--wasm-dir <path>`    | _required_ | Directory produced by `cargo build --release` for `wasm32-unknown-unknown` |
| `--tolerance-pct <n>`  | `5`        | Maximum allowed percent growth before a contract is reported as regressing |
| `--update-baseline`    | off        | Write the measured sizes back to the baseline file (creating entries for new contracts, overwriting existing entries with the new value) |
| `--fail-on-new`        | off        | With `--update-baseline=false` (the default), fail when a `.wasm` is present that has no entry in the baseline (new contract needs an explicit baseline) |
| `--allow-missing`      | off        | Skip baseline entries whose `.wasm` file is missing (instead of failing) |
| `--report <path>`      | stdout     | Write the markdown report to this file in addition to stdout |

Exit code is `0` if every contract is within tolerance and the baseline is
consistent. `1` if any contract regresses, has a missing `.wasm`, or has a
`.wasm` without a baseline entry (under `--fail-on-new`).

## Baseline Format

See [`../../benchmarks/wasm_sizes.json`](../../benchmarks/wasm_sizes.json).
Each contract entry records:

```json
{
  "size_bytes": 12345,
  "sha256": "sha256:…",
  "captured_at": "YYYY-MM-DD"
}
```

The `sha256` field is optional but recommended: it lets the tool warn if a
baseline entry's recorded hash doesn't match the actual artifact (a strong
signal that the baseline was copy/pasted or the artifact was rebuilt from a
different source than the original capture).

## Update Flow

When a PR legitimately changes a contract's compiled size:

1. Run the tool locally: `cargo run --release -- --baseline … --wasm-dir …`
2. Confirm the reported deltas are intentional.
3. Re-run with `--update-baseline` to refresh the committed baseline.
4. Commit `benchmarks/wasm_sizes.json` in the **same PR** as the source change.

CI will fail without this update — a size regression without a refreshed
baseline is treated as a build failure.

## Tests

```bash
cargo test
```

Coverage targets the baseline IO/percent math, inventory scanning, and report
formatting. Each module has dedicated unit tests; `tests/integration.rs` covers
end-to-end flows (pass, regress, missing, new contract, update-baseline).
