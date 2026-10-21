#!/usr/bin/env bash
#
# Self-test for scripts/check-contract-no-traps.sh.
#
# Asserts that the guard:
#   * accepts clean, typed-only deployable source,
#   * accepts an in-file `#[cfg(test)]` module that uses the `assert!` family,
#   * accepts `mock_contract.rs` (exempt by name),
#   * rejects newly introduced `assert!`, `assert_eq!`, and `assert_ne!`,
#   * accepts the repository's own contract source.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
guard="$here/../check-contract-no-traps.sh"
fixtures="$here/fixtures/trap-check"

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  exit 1
}

# Clean, typed-only deployable source must pass.
bash "$guard" "$fixtures/clean/src" || fail "clean source should pass"

# An in-file #[cfg(test)] module may use the assert! family.
bash "$guard" "$fixtures/test-module/src" || fail "test module should be exempt"

# mock_contract.rs is exempt even though it contains an untyped panic.
bash "$guard" "$fixtures/mock-contract/src" || fail "mock_contract.rs should be exempt"

# Newly introduced assert!/assert_eq!/assert_ne! in deployable source must fail,
# and the original unwrap/expect/panic patterns must keep failing.
for case in assertion assert-eq legacy-trap; do
  if bash "$guard" "$fixtures/$case/src" >/dev/null 2>&1; then
    fail "expected the '$case' fixture to be rejected"
  fi
done

# The repository's own contract source must be clean.
bash "$guard" "$here/../../onchain/contracts/stello_pay_contract/src" ||
  fail "repository contract source should be clean"

printf 'check-contract-no-traps self-test passed\n'
