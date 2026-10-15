#!/usr/bin/env bash
# DEMO ONLY. Builds the program if needed, starts a fresh local validator with it, and serves the UI.
set -euo pipefail
cd "$(dirname "$0")"
PROGRAM_ID=$(solana-keygen pubkey target/deploy/mmf_demo-keypair.json)

[ -f target/deploy/mmf_demo.so ] || anchor build
[ -d app/node_modules ] || (cd app && npm ci)

pkill -f "solana-test-validator.*mmf_demo" 2>/dev/null || true
rm -f app/.demo-state.json
solana-test-validator --reset --quiet --ledger .ledger \
  --bpf-program "$PROGRAM_ID" target/deploy/mmf_demo.so > validator.log 2>&1 &
VALIDATOR=$!
trap 'kill $VALIDATOR 2>/dev/null' EXIT

echo -n "Starting local validator"
until solana cluster-version -u http://127.0.0.1:8899 >/dev/null 2>&1; do echo -n "."; sleep 1; done
echo " ready."

cd app && node server.mjs
