#!/usr/bin/env bash
set -euo pipefail

NETWORK=${1:-testnet}
DEPLOYER=deployer
OUTPUT_DIR="deployments"
OUTPUT_FILE="$OUTPUT_DIR/$NETWORK.json"

mkdir -p "$OUTPUT_DIR"

echo "🔨 Building contracts..."
stellar contract build

echo "🚀 Deploying to $NETWORK..."

TREASURY_ID=$(stellar contract deploy \
  --wasm target/wasm32-unknown-unknown/release/coopfin_treasury.wasm \
  --source "$DEPLOYER" \
  --network "$NETWORK" \
  2>&1 | tail -1)

LOAN_ID=$(stellar contract deploy \
  --wasm target/wasm32-unknown-unknown/release/coopfin_loan.wasm \
  --source "$DEPLOYER" \
  --network "$NETWORK" \
  2>&1 | tail -1)

VOTING_ID=$(stellar contract deploy \
  --wasm target/wasm32-unknown-unknown/release/coopfin_voting.wasm \
  --source "$DEPLOYER" \
  --network "$NETWORK" \
  2>&1 | tail -1)

GOVERNANCE_ID=$(stellar contract deploy \
  --wasm target/wasm32-unknown-unknown/release/coopfin_governance.wasm \
  --source "$DEPLOYER" \
  --network "$NETWORK" \
  2>&1 | tail -1)

DIVIDEND_ID=$(stellar contract deploy \
  --wasm target/wasm32-unknown-unknown/release/coopfin_dividend.wasm \
  --source "$DEPLOYER" \
  --network "$NETWORK" \
  2>&1 | tail -1)

cat > "$OUTPUT_FILE" << JSON
{
  "network": "$NETWORK",
  "deployedAt": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "contracts": {
    "treasury":   "$TREASURY_ID",
    "loan":       "$LOAN_ID",
    "voting":     "$VOTING_ID",
    "governance": "$GOVERNANCE_ID",
    "dividend":   "$DIVIDEND_ID"
  }
}
JSON

echo "✅ Deployed! Addresses written to $OUTPUT_FILE"
cat "$OUTPUT_FILE"
