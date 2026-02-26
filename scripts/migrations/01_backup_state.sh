#!/usr/bin/env bash
# =============================================================================
# Migration script: Pre-upgrade state backup
# =============================================================================
#
# @title Pre-upgrade state backup
# @notice Backs up contract configuration and state before upgrading so that
#         you can verify post-upgrade consistency and have a record for rollback.
# @dev    Writes meta.json, contract_id.txt; uses stellopay-cli export if available.
#
# Usage:
#   ./scripts/migrations/01_backup_state.sh
#
# Environment:
#   CONTRACT_ID   (required) Contract instance address (e.g. C...).
#   NETWORK       (optional) testnet | mainnet | futurenet. Default: testnet.
#   RPC_URL       (optional) Soroban RPC URL.
#   BACKUP_DIR    (optional) Output directory. Default: ./backups/<timestamp>.
#
# Exit: 0 on success; 1 if CONTRACT_ID is missing or CLI not found.
# =============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
BACKUP_DIR="${BACKUP_DIR:-$REPO_ROOT/backups/$(date +%Y%m%d_%H%M%S)}"
CONTRACT_ID="${CONTRACT_ID:-}"
NETWORK="${NETWORK:-testnet}"
RPC_URL="${RPC_URL:-https://soroban-testnet.stellar.org}"

if [[ -z "$CONTRACT_ID" ]]; then
  echo "Error: CONTRACT_ID is required. Set it to the payroll contract address (e.g. C...)."
  exit 1
fi

mkdir -p "$BACKUP_DIR"
echo "Backup directory: $BACKUP_DIR"

# Detect CLI (stellar or soroban)
CLI="stellar"
if ! command -v stellar &>/dev/null; then
  if command -v soroban &>/dev/null; then
    CLI="soroban"
  else
    echo "Error: Neither 'stellar' nor 'soroban' CLI found. Install Stellar CLI."
    exit 1
  fi
fi

# Save metadata
cat > "$BACKUP_DIR/meta.json" << EOF
{
  "contract_id": "$CONTRACT_ID",
  "network": "$NETWORK",
  "rpc_url": "$RPC_URL",
  "backup_time_iso": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "script": "01_backup_state.sh"
}
EOF

# Export owner (if your CLI supports reading contract state)
# Soroban/Stellar CLI may not expose raw storage; document expected keys for manual backup.
echo "Contract ID: $CONTRACT_ID" > "$BACKUP_DIR/contract_id.txt"
echo "Network: $NETWORK" >> "$BACKUP_DIR/contract_id.txt"
echo "RPC: $RPC_URL" >> "$BACKUP_DIR/contract_id.txt"

# If stellopay-cli is available, export state and payrolls (see docs/dev-tools)
if command -v stellopay-cli &>/dev/null; then
  stellopay-cli export state --contract "$CONTRACT_ID" --output "$BACKUP_DIR/state.json" 2>/dev/null || true
  stellopay-cli export payrolls --contract "$CONTRACT_ID" --output "$BACKUP_DIR/payrolls.json" 2>/dev/null || true
fi

echo "Backup completed. Files in: $BACKUP_DIR"
echo "Store the current WASM hash for this contract (from deploy records) for rollback."
echo "Next: run 02_build_and_install_wasm.sh to build and install the new WASM."
