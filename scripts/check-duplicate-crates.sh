#!/usr/bin/env bash
# Research R1: fail if the workspace resolves two different versions of solana-address within the
# same major version (a real type conflict). solana-address 1.1.x re-exports 2.x on purpose, so one
# 1.x plus one 2.x is expected and allowed.
set -euo pipefail
cd "$(dirname "$0")/.."
versions="$(cargo tree --workspace --prefix none -e normal,build,dev 2>/dev/null \
  | grep -Eo '^solana-address v[0-9]+\.[0-9]+\.[0-9]+' | sort -u | awk '{print $2}')"
echo "solana-address versions: ${versions//$'\n'/ }"
dupes="$(printf '%s\n' "$versions" | sed 's/^v//' | cut -d. -f1 | sort | uniq -d)"
if [ -n "$dupes" ]; then
  echo "duplicate solana-address versions within major ${dupes//$'\n'/ }"; exit 1
fi
