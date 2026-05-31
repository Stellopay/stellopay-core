# WASM Build Targets

## Required target: `wasm32-unknown-unknown`

All Soroban smart contracts in this repository must be compiled for the
`wasm32-unknown-unknown` target. This is the only target supported by
Soroban SDK 23.4.1 and the Stellar network.

## Why not `wasm32v1-none`?

`wasm32v1-none` is a newer, more restrictive target that:

- Lacks the WASI imports expected by the Soroban host environment.
- Produces binaries that fail on-network deployment with linker errors or
  silent incompatibilities.
- Is **not** accepted by `stellar contract build` or `stellar contract deploy`.

## CI configuration

`.github/workflows/contracts.yml` installs the correct target before any
build or test step:

```yaml
- name: Add WASM target
  run: rustup target add wasm32-unknown-unknown
```

`stellar contract build` automatically passes `--target wasm32-unknown-unknown`
internally, so no extra flags are needed in the build steps.

## Local development

```bash
rustup target add wasm32-unknown-unknown
stellar contract build
```

To verify the installed targets:

```bash
rustup target list --installed | grep wasm
# expected: wasm32-unknown-unknown
```

## Auditing for regressions

Search for any accidental re-introduction of the wrong target:

```bash
grep -r "wasm32v1-none" .github/ onchain/
```

This should return no results. The CI workflow and all build scripts must
reference `wasm32-unknown-unknown` exclusively.
