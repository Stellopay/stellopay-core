# Build only the payroll contract WASM (no host build).
# Use this on Windows when the GNU toolchain hits "export ordinal too large" during
# cargo test or normal build. See docs/windows-build.md.
$ErrorActionPreference = "Stop"
$RepoRoot = (Get-Item $PSScriptRoot).Parent.Parent.FullName
$ContractDir = Join-Path $RepoRoot "onchain\contracts\stello_pay_contract"
Set-Location $ContractDir
Write-Host "Building stello_pay_contract for wasm32-unknown-unknown (release)..."
cargo build -p stello_pay_contract --target wasm32-unknown-unknown --release
$wasm = Join-Path $ContractDir "target\wasm32-unknown-unknown\release\stello_pay_contract.wasm"
if (Test-Path $wasm) {
    Write-Host "WASM built: $wasm"
} else {
    Write-Error "WASM not found at $wasm"
}
