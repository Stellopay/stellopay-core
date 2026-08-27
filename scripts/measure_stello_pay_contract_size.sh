#!/usr/bin/env bash
# Measure the stello_pay_contract wasm variants described in issue #1264.
#
# This is an intentionally manual tool, not a CI gate. It creates disposable
# copies of a git revision so each variant is built with the same source tree,
# Cargo.lock starting point, Stellar CLI, and release profile.

set -euo pipefail

usage() {
    cat <<'EOF'
Usage: measure_stello_pay_contract_size.sh [options]

Options:
  --baseline-ref REF  Revision containing the pre-change contract.
                      Defaults to HEAD^ when HEAD has a parent.
  --keep              Keep the temporary build directory for inspection.
  -h, --help          Show this help.

The command must be run from inside the repository and requires the Stellar
CLI, Cargo, and a git checkout. It prints a tab-separated report suitable for
copying into the issue or pull request description.
EOF
}

die() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

repo_root="$(git rev-parse --show-toplevel 2>/dev/null)" || die "run inside a git checkout"
baseline_ref=""
keep_temp=false

while (($# > 0)); do
    case "$1" in
        --baseline-ref)
            (($# >= 2)) || die "--baseline-ref requires a revision"
            baseline_ref="$2"
            shift 2
            ;;
        --keep)
            keep_temp=true
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            die "unknown option: $1"
            ;;
    esac
done

command -v cargo >/dev/null 2>&1 || die "cargo is required"
command -v stellar >/dev/null 2>&1 || die "stellar CLI is required"
command -v shasum >/dev/null 2>&1 || die "shasum is required"
command -v git >/dev/null 2>&1 || die "git is required"

if [[ -z "$baseline_ref" ]]; then
    baseline_ref="$(git rev-parse HEAD^)" || die "HEAD has no parent; pass --baseline-ref"
fi

git rev-parse --verify "$baseline_ref^{commit}" >/dev/null \
    || die "baseline revision does not resolve: $baseline_ref"

contract_rel="onchain/contracts/stello_pay_contract"
contract_manifest="$contract_rel/Cargo.toml"
contract_source="$contract_rel/src/lib.rs"
workspace_manifest="onchain/Cargo.toml"
wasm_rel="target/wasm32v1-none/release/stello_pay_contract.wasm"
limit=131072

temp_root="$(mktemp -d "${TMPDIR:-/tmp}/stello-pay-size.XXXXXX")"
cleanup() {
    if "$keep_temp"; then
        printf '# temporary variants kept at %s\n' "$temp_root" >&2
    else
        rm -rf "$temp_root"
    fi
}
trap cleanup EXIT

archive_revision() {
    local revision="$1"
    local destination="$2"
    mkdir -p "$destination"
    git archive "$revision" | tar -x -C "$destination"
}

remove_unused_dependencies() {
    local manifest="$1"
    perl -0pi -e 's/^soroban-token-sdk\s*=.*\n//m; s/^stellar-access\s*=.*\n//m; s/^stellar-contract-utils\s*=.*\n//m; s/^stellar-macros\s*=.*\n//m; s/^stellar-tokens\s*=.*\n//m' "$manifest"
}

gate_mock_module() {
    local source="$1"
    perl -0pi -e 's/\#\[cfg\(not\(target_family = "wasm"\)\)\]/#[cfg(test)]/' "$source"
}

use_full_lto() {
    local manifest="$1"
    perl -0pi -e 's/^lto\s*=\s*true$/lto = "fat"/m' "$manifest"
}

sorted_exports() {
    local wasm="$1"
    stellar contract info interface --wasm "$wasm" 2>/dev/null \
        | grep -oE 'fn [a-z_0-9]+' \
        | sort
}

build_variant() {
    local name="$1"
    local revision="$2"
    local remove_deps="$3"
    local gate_mock="$4"
    local variant="$temp_root/$name"
    local wasm
    local size
    local hash
    local export_count
    local export_digest

    archive_revision "$revision" "$variant"
    use_full_lto "$variant/$workspace_manifest"

    if "$remove_deps"; then
        remove_unused_dependencies "$variant/$contract_manifest"
    fi
    if "$gate_mock"; then
        gate_mock_module "$variant/$contract_source"
    fi

    (
        cd "$variant/onchain"
        stellar contract build --package stello_pay_contract >/dev/null
    )

    wasm="$variant/onchain/$wasm_rel"
    [[ -f "$wasm" ]] || die "$name did not produce $wasm_rel"
    size="$(stat -f '%z' "$wasm" 2>/dev/null || stat -c '%s' "$wasm")"
    hash="$(shasum -a 256 "$wasm" | awk '{print $1}')"
    export_count="$(sorted_exports "$wasm" | wc -l | tr -d ' ')"
    export_digest="$(sorted_exports "$wasm" | shasum -a 256 | awk '{print $1}')"

    printf '%s\t%s\t%s\t%s\t%s\t%s\n' \
        "$name" "$size" "$hash" "$export_count" "$export_digest" "$wasm"
}

printf '# variant\tbytes\tsha256\texports\texport-list-sha256\tartifact\n'
printf '# limit\t%s\n' "$limit"

baseline_line="$(build_variant baseline "$baseline_ref" false false)"
printf '%s\n' "$baseline_line"
baseline_bytes="$(printf '%s\n' "$baseline_line" | awk -F '\t' '{print $2}')"

measure_and_print() {
    local name="$1"
    local revision="$2"
    local remove_deps="$3"
    local gate_mock="$4"
    local line
    local bytes
    local delta
    line="$(build_variant "$name" "$revision" "$remove_deps" "$gate_mock")"
    bytes="$(printf '%s\n' "$line" | awk -F '\t' '{print $2}')"
    delta=$((bytes - baseline_bytes))
    printf '%s\tdelta=%+d\n' "$line" "$delta"
}

measure_and_print deps-only "$baseline_ref" true false
measure_and_print mock-gate-only "$baseline_ref" false true
measure_and_print combined "$baseline_ref" true true

final_bytes="$(build_variant final "$HEAD" false false | awk -F '\t' '{print $2}')"
headroom=$((limit - final_bytes))
printf '# final-headroom-bytes\t%s\n' "$headroom"
printf '# final-headroom-percent\t%.2f%%\n' "$(awk -v h="$headroom" -v l="$limit" 'BEGIN { print (h / l) * 100 }')"
