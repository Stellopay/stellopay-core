#!/usr/bin/env bash
#
# Reject untyped traps in deployable payroll contract source.
#
# The guard fails on `.unwrap()`, `.expect(`, `panic!`, and the `assert!`
# family (`assert!`, `assert_eq!`, `assert_ne!`). Recoverable preconditions must
# surface a typed `PayrollError` — returned from the entrypoint or raised with
# `panic_with_error!` — so off-chain callers can branch on a stable
# discriminant. The `assert!` family aborts with an untyped string message and
# is therefore rejected for exactly the same reason as `.unwrap()`/`.expect()`/
# `panic!`.
#
# Exemptions:
#   * Integration-test sources (`src/tests/**`, `tests/**`).
#   * In-file `#[cfg(test)]` modules, e.g. the discriminant-stability test at the
#     bottom of `storage.rs`.
#   * `mock_contract.rs`, a native-only test fixture whose legacy traps are
#     intentionally retained to model an external upgradeable-contract failure.
#
# Usage: scripts/check-contract-no-traps.sh [contract-src-dir]
set -euo pipefail

contract_src="${1:-onchain/contracts/stello_pay_contract/src}"

if [[ ! -d "$contract_src" ]]; then
  printf 'contract source directory not found: %s\n' "$contract_src" >&2
  exit 2
fi

violations=$(
  find "$contract_src" -type f -name '*.rs' \
    ! -path "$contract_src/tests/*" \
    ! -path "$contract_src/*/tests/*" \
    ! -name 'mock_contract.rs' \
    -print0 |
    xargs -0 -r awk '
      FNR == 1 { pending_cfg_test = 0; in_test_module = 0; test_depth = 0 }

      in_test_module {
        test_depth += gsub(/\{/, "{") - gsub(/\}/, "}")
        if (test_depth <= 0) in_test_module = 0
        next
      }

      /^[[:space:]]*#\[cfg\(test\)\]/ {
        pending_cfg_test = 1
        if ($0 ~ /mod[[:space:]]/) {
          in_test_module = 1
          test_depth = gsub(/\{/, "{") - gsub(/\}/, "}")
          pending_cfg_test = 0
        }
        next
      }

      pending_cfg_test {
        if ($0 ~ /^[[:space:]]*mod[[:space:]]+[[:alnum:]_]+[[:space:]]*\{/) {
          in_test_module = 1
          test_depth = gsub(/\{/, "{") - gsub(/\}/, "}")
          pending_cfg_test = 0
          next
        }
        # Attributes, comments, and blank lines may sit between `#[cfg(test)]`
        # and the `mod` keyword; keep waiting for it.
        if ($0 !~ /^[[:space:]]*(#\[|\/\/|$)/) pending_cfg_test = 0
        if (pending_cfg_test) next
      }

      /\.unwrap\(\)|\.expect\(|panic!|assert!|assert_eq!|assert_ne!/ {
        print FILENAME ":" FNR ":" $0
      }
    '
)

if [[ -n "$violations" ]]; then
  printf 'Unguarded trap found in deployable payroll source:\n%s\n' "$violations" >&2
  exit 1
fi
