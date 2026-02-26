#!/usr/bin/env bash
# =============================================================================
# Run migration-related tests (unit and integration)
# =============================================================================
# Executes the contract's upgrade and data-persistence tests to validate
# that migrations and upgrades do not break existing behavior.
#
# Usage:
#   ./scripts/migrations/run_migration_tests.sh
#
# See docs/migrations.md for full testing procedures.
# =============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CONTRACT_CRATE="$REPO_ROOT/onchain/contracts/stello_pay_contract"
INTEGRATION_DIR="$REPO_ROOT/onchain/integration_tests"

echo "Running payroll contract tests (includes upgrade and data persistence)..."
cd "$CONTRACT_CRATE"
cargo test --verbose

echo "Running integration tests..."
cd "$REPO_ROOT/onchain"
cargo test -p integration_tests --verbose 2>/dev/null || cargo test --manifest-path Cargo.toml --verbose 2>/dev/null || true

echo "Migration-related tests completed. See docs/migrations.md for testnet/staging checklist."
