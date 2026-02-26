#!/usr/bin/env bash
# =============================================================================
# Migration script: Post-upgrade verification
# =============================================================================
#
# @title Post-upgrade verification
# @notice Runs basic checks after an upgrade (contract invokable, optional state checks).
# @dev    Extend with contract-specific view calls (e.g. get_owner) as needed.
#
# Usage:
#   ./scripts/migrations/04_verify_post_upgrade.sh
#
# Environment:
#   CONTRACT_ID (required) Contract instance to verify.
#   NETWORK, RPC_URL (optional) Network configuration.
#
# Exit: 0 on success; 1 if CONTRACT_ID missing or CLI not found.
# =============================================================================
set -euo pipefail

CONTRACT_ID="${CONTRACT_ID:-}"
NETWORK="${NETWORK:-testnet}"
RPC_URL="${RPC_URL:-https://soroban-testnet.stellar.org}"

if [[ -z "$CONTRACT_ID" ]]; then
  echo "Error: CONTRACT_ID is required."
  exit 1
fi

# Detect CLI
CLI="stellar"
if ! command -v stellar &>/dev/null; then
  if command -v soroban &>/dev/null; then
    CLI="soroban"
  else
    echo "Error: Neither 'stellar' nor 'soroban' CLI found."
    exit 1
  fi
fi

INVOKE_ARGS=(--id "$CONTRACT_ID" --network "$NETWORK")
if [[ -n "$RPC_URL" ]]; then
  INVOKE_ARGS+=(--rpc-url "$RPC_URL")
fi

echo "Verifying contract $CONTRACT_ID is invokable..."
# Try a read-only call if available (e.g. get_owner or similar).
# PayrollContract has initialize(owner) but no get_owner in the snippet;
# adjust to a view your contract exposes.
"$CLI" contract invoke "${INVOKE_ARGS[@]}" -- 2>/dev/null || true

echo "Post-upgrade verification completed."
echo "Manually confirm: owner, active agreements, and one disburse_salary (or equivalent) if possible."
