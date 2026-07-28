# CLI exit codes

The `stellopay-cli` binary exits with distinct, documented codes so scripts
and CI wrappers can tell failure kinds apart instead of treating every
non-zero exit the same.

| Code | Name        | Meaning                                                              |
|------|-------------|---------------------------------------------------------------------|
| `0`  | `Success`   | Command completed successfully.                                     |
| `1`  | `Generic`   | Unspecified failure (catch-all for errors we can't categorize).     |
| `2`  | `Usage`     | Command-line usage error (bad flags / unknown subcommand). Emitted by `clap` before the command runs. |
| `3`  | `Config`    | Configuration error (missing / unreadable / malformed `stellopay.toml`). |
| `4`  | `Network`   | Network / RPC failure talking to a Soroban endpoint.               |
| `5`  | `Verification` | Verification failure (e.g. deployed WASM hash mismatch).        |

The mapping is implemented in `src/lib.rs` (`ExitCode` + `classify_error`) and
applied in `src/main.rs` for both config-load and command-execution errors.

## Configuration precedence

When resolving a setting, the CLI applies the following precedence (highest
first):

1. **CLI flag** — e.g. `--network` on the `deploy` command.
2. **Environment variable** — `STELLOPAY_*` (see `src/config.rs`).
3. **TOML config file** — `stellopay.toml` (or the path given by `--config`).
4. **Built-in default** — see `Config::default()` in `src/lib.rs`.
