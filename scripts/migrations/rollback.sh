#!/usr/bin/env bash
# =============================================================================
# Migration script: Rollback to previous WASM
# =============================================================================
#
# @title Rollback to previous WASM
# @notice Upgrades the contract back to a previous WASM hash (same contract ID).
# @dev    Invokes upgrade(previous_wasm_hash) with owner auth. Storage unchanged.
#         Keep PREVIOUS_WASM_HASH from pre-upgrade backup or deploy records.
#
# Usage:
#   ./scripts/migrations/rollback.sh <PREVIOUS_WASM_HASH>
#
# Params:
#   PREVIOUS_WASM_HASH  Last known-good WASM hash (from backup).
#
# Environment:
#   CONTRACT_ID     (required) Contract to roll back.
#   SOURCE_ACCOUNT  (required) Owner secret key / identity.
#   NETWORK, RPC_URL (optional) Network configuration.
#
# Exit: 0 on success; 1 if required env/args missing or invoke fails.
# =============================================================================
set -euo pipefail

PREVIOUS_WASM_HASH="${1:-}"

if [[ -z "$PREVIOUS_WASM_HASH" ]]; then
  echo "Error: PREVIOUS_WASM_HASH required. Pass the last known-good WASM hash (from backup)."
  exit 1
fi

CONTRACT_ID="${CONTRACT_ID:-}"
SOURCE_ACCOUNT="${SOURCE_ACCOUNT:-}"
NETWORK="${NETWORK:-testnet}"
RPC_URL="${RPC_URL:-https://soroban-testnet.stellar.org}"

if [[ -z "$CONTRACT_ID" ]] || [[ -z "$SOURCE_ACCOUNT" ]]; then
  echo "Error: CONTRACT_ID and SOURCE_ACCOUNT are required for rollback."
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

echo "Rolling back contract $CONTRACT_ID to WASM hash: $PREVIOUS_WASM_HASH"

# Invoke contract upgrade with previous WASM hash (owner auth required)
"$CLI" contract invoke \
  --id "$CONTRACT_ID" \
  --network "$NETWORK" \
  --source-account "$SOURCE_ACCOUNT" \
  ${RPC_URL:+--rpc-url "$RPC_URL"} \
  -- \
  upgrade \
  --new_wasm_hash "$PREVIOUS_WASM_HASH"

echo "Rollback completed. Run 04_verify_post_upgrade.sh to verify."
