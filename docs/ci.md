# CI / Continuous Integration

This document describes the GitHub Actions CI setup for the `stellopay-core`
repository, with a focus on the Rust build-cache strategy used to keep
pipeline runtimes short.

---

## Workflows

| File | Runner | Purpose |
|------|--------|---------|
| `.github/workflows/ci.yml` | `macos-latest` | Full Soroban contract build + tests |
| `.github/workflows/contracts.yml` | `ubuntu-latest` | Native and wasm32 workspace builds |
| `.github/workflows/security-scan.yml` | `ubuntu-latest` | `cargo-audit` + Clippy static analysis |

---

## Rust Dependency Caching

All three workflows use `actions/cache@v4` to persist Cargo artefacts between
runs.  The directories that are cached are:

```
~/.cargo/registry/index   # crates.io sparse index
~/.cargo/registry/cache   # downloaded .crate tarballs
~/.cargo/git/db           # git-sourced dependencies
onchain/target            # compiled build artefacts
```

### Cache key strategy

```
<runner-os>-cargo-<job-suffix>-<sha256 of onchain/Cargo.lock>
```

| Component | Why |
|-----------|-----|
| `runner.os` | Prevents cross-platform cache pollution |
| job suffix (`native`, `wasm`, `security`) | Keeps native and wasm artefacts separate |
| `hashFiles('onchain/Cargo.lock')` | Invalidates the cache when any dependency version changes |

A `restore-keys` fallback (without the `Cargo.lock` hash) lets a new run
reuse a stale cache and only rebuild what changed, rather than starting cold.

### Security boundary for fork pull requests

GitHub Actions does not allow pull requests from forks to write to the
repository's cache (only trusted pushes to `main` can populate it).  Fork PRs
read from the best matching `restore-keys` entry, so they still benefit from a
warm registry cache without being able to inject malicious build artefacts into
a trusted run.

---

## Adding a new workflow

1. Add `actions/checkout@v4` as the first step.
2. Add `dtolnay/rust-toolchain@stable` (or the curl-based installer for macOS).
3. Copy the cache step from an existing workflow, choosing a unique job suffix
   so artefacts remain isolated.
4. Place the cache step **after** toolchain installation and **before** any
   `cargo build` / `cargo test` invocations.

---

## Local reproduction

```bash
# Build the entire workspace natively
cargo build --workspace --manifest-path onchain/Cargo.toml

# Build for wasm32 (requires the target to be installed first)
rustup target add wasm32v1-none
cargo build --workspace --manifest-path onchain/Cargo.toml \
  --target wasm32v1-none --release

# Run all tests
cargo test --workspace --manifest-path onchain/Cargo.toml
```
