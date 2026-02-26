# Building on Windows

If you see a linker error like **"export ordinal too large: 73832"** (or similar) when running `cargo test` or `cargo build` in the `onchain` workspace, you are hitting a **Windows PE export table limit** when using the **GNU toolchain** (MinGW). The combined dependency tree (soroban-sdk, stellar-*, crypto libs) exports more than 65,535 symbols, which MinGW's linker cannot handle.

## Option B (recommended): Build only WASM; run tests in WSL or CI

Keep the **GNU** toolchain and build only the contract WASM on Windows. Run tests in WSL or rely on CI (e.g. GitHub Actions).

### 1. Add the wasm target

```powershell
rustup target add wasm32-unknown-unknown
```

### 2. Build the contract WASM

From the **repo root**:

```powershell
.\scripts\migrations\build_wasm_only.ps1
```

Or manually:

```powershell
cd onchain\contracts\stello_pay_contract
cargo build -p stello_pay_contract --target wasm32-unknown-unknown --release
```

The built WASM is at:

`onchain\contracts\stello_pay_contract\target\wasm32-unknown-unknown\release\stello_pay_contract.wasm`

### 3. Running tests

- **WSL:** Open the repo in WSL and run `cargo test` from `onchain/contracts/stello_pay_contract` (or use `./scripts/migrations/run_migration_tests.sh` from the repo root in WSL).
- **CI:** GitHub Actions (and other Linux/macOS CI) run the full test suite; no extra steps on your side.

---

## Option A: Use the MSVC toolchain (full local builds and tests on Windows)

If you prefer to run `cargo test` and `stellar contract build` directly on Windows without WSL:

1. Install [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with the **"Desktop development with C++"** workload.
2. Switch to the MSVC toolchain:
   ```powershell
   rustup default stable-x86_64-pc-windows-msvc
   rustup target add wasm32-unknown-unknown
   ```
3. From `onchain\contracts\stello_pay_contract`: `cargo test` and `stellar contract build` will then work.

---

## Summary

| Goal                     | Option B (GNU + WASM only)   | Option A (MSVC)              |
|--------------------------|------------------------------|-----------------------------|
| Build contract WASM     | Yes (script or wasm target)  | Yes                         |
| Run `cargo test` locally| WSL or CI                    | Yes, on Windows             |
| No VS Build Tools        | Yes                          | No (required for MSVC)      |
