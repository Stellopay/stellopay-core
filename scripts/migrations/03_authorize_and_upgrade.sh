#!/usr/bin/env bash
# =============================================================================
# Migration script: Authorize and perform contract upgrade
# =============================================================================
#
# @title Authorize and upgrade contract
# @notice Invokes the contract's upgrade(new_wasm_hash) with owner auth.
# @dev    Contract enforces owner via UpgradeableInternal and calls
#         deployer().update_current_contract_wasm(new_wasm_hash). Contract ID unchanged.
#
# Usage:
#   ./scripts/migrations/03_authorize_and_upgrade.sh <NEW_WASM_HASH>
#
# Params:
#   NEW_WASM_HASH   First argument or env NEW_WASM_HASH (from 02_build_and_install_wasm.sh).
#
# Environment:
#   CONTRACT_ID     (required) Contract instance to upgrade.
#   SOURCE_ACCOUNT  (required) Owner secret key / identity.
#   NETWORK, RPC_URL (optional) Network configuration.
#
# Exit: 0 on success; 1 if required env/args missing or invoke fails.
# =============================================================================
set -euo pipefail

NEW_WASM_HASH="${1:-$NEW_WASM_HASH}"
CONTRACT_ID="${CONTRACT_ID:-}"
SOURCE_ACCOUNT="${SOURCE_ACCOUNT:-}"
NETWORK="${NETWORK:-testnet}"
RPC_URL="${RPC_URL:-https://soroban-testnet.stellar.org}"

if [[ -z "$NEW_WASM_HASH" ]]; then
  echo "Error: NEW_WASM_HASH required. Pass as first argument or set env (e.g. from .env.migration)."
  exit 1
fi

if [[ -z "$CONTRACT_ID" ]]; then
  echo "Error: CONTRACT_ID is required."
  exit 1
fi

if [[ -z "$SOURCE_ACCOUNT" ]]; then
  echo "Error: SOURCE_ACCOUNT (owner) is required for authorize_upgrade."
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

INVOKE_ARGS=(
  --id "$CONTRACT_ID"
  --network "$NETWORK"
  --source-account "$SOURCE_ACCOUNT"
)
if [[ -n "$RPC_URL" ]]; then
  INVOKE_ARGS+=(--rpc-url "$RPC_URL")
fi

# PayrollContract uses UpgradeableInternal: upgrade is performed by invoking
# the contract's upgrade function with the new WASM hash; the contract checks
# owner auth and calls deployer().update_current_contract_wasm(new_wasm_hash).
# If your contract has a separate authorize_upgrade step, run it first.
echo "Authorizing and performing upgrade (owner must sign)..."
"$CLI" contract invoke "${INVOKE_ARGS[@]}" \
  -- \
  upgrade \
  --new_wasm_hash "$NEW_WASM_HASH"

echo "Upgrade completed. Run 04_verify_post_upgrade.sh to verify."
