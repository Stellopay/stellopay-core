# Contract size and build-input hygiene

This document records the measurements for the dependency and mock-module
cleanup in issue #1264. Measurements were taken from clean detached worktrees
with the same `stellar contract build --package stello_pay_contract` command.
The release profile uses full LTO (`lto = "fat"`) so the comparison is
deterministic for the repository's current Soroban toolchain.

## Measurements

The optimized wasm limit used by the issue is 131,072 bytes.

| Variant | Bytes | Delta vs baseline | SHA-256 prefix |
| --- | ---: | ---: | --- |
| Baseline (`cfg(not(target_family = "wasm"))`, five dependencies) | 120,510 | — | `36919648eaf431c6` |
| Five dependencies removed only | 120,510 | 0 | `2b51f32a35109efc` |
| Mock gate changed to `cfg(test)` only | 120,510 | 0 | `36919648eaf431c6` |
| Combined change | 120,510 | 0 | `2b51f32a35109efc` |

The final headroom is therefore **10,562 bytes**, or **8.06%** of the
131,072-byte limit. The unused dependencies were not linked into the wasm,
so their removal changes the dependency graph and lockfile without changing
the optimized artifact size. The stricter mock gate is likewise a native
build-hygiene improvement; the existing wasm-target gate had already excluded
the module from the deployed artifact.

The issue text asks for a result strictly smaller than 120,510 bytes. On the
current clean toolchain these build-input-only changes reproduce exactly
120,510 bytes. No contract logic, exported function, or ABI was altered to
manufacture a smaller number; the equality is reported deliberately for
reviewer visibility.

## Mock gating choice

`mock_contract` is guarded with `#[cfg(test)]`. This is the narrowest gate for
the library's own upgrade tests and guarantees that a normal dependency build
cannot expose the mock contract types. The reentrancy test is an integration
test, which is compiled as a separate crate and cannot see `cfg(test)` items
from the library. Its small callback mock now lives in
`tests/support/mod.rs`, where it remains test-owned and is still registered and
called end-to-end by `tests/test_reentrancy.rs`.

## ABI verification

The baseline and final artifacts both report 77 exported function names from:

```text
stellar contract info interface \
  --wasm target/wasm32v1-none/release/stello_pay_contract.wasm
```

The sorted export-name digest for both artifacts is
`c33f22bef6fec73647d8b6c93d76a69cbba7a5be4c654302e528eeb631c4542a`.
No contract behavior or public function signature was changed.

## Dependency-hygiene decision

The five zero-reference dependencies were removed from both
`stello_pay_contract/Cargo.toml` and `onchain/Cargo.lock`. A new CI dependency
hygiene job was not added: the issue marks the CI size guard as out of scope,
and introducing a new third-party tool such as `cargo-udeps` or
`cargo-machete` would add toolchain and maintenance policy beyond this focused
build-input change. The regression tests in `tests/build_input_hygiene.rs`
still make accidental reintroduction visible during the existing test run.
