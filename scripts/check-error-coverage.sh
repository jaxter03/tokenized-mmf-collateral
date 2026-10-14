#!/usr/bin/env bash
# SC-004: every error variant declared in an #[error_code] enum under programs/*/src/errors.rs
# must be asserted by at least one test in tests/tests/. A variant counts as covered when some
# test file references it as `EnumName::Variant`, so shared names (e.g. Unauthorized) are checked
# per program. A variant preceded by a `// retired` comment (kept only so later codes keep their
# numbers, and returned by nothing) is skipped. Exits 1 and lists the uncovered variants otherwise.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
shopt -s nullglob
error_files=("$root"/programs/*/src/errors.rs)
test_files=("$root"/tests/tests/*.rs)

if [ ${#error_files[@]} -eq 0 ]; then
  echo "check-error-coverage: no programs/*/src/errors.rs yet; nothing to check"
  exit 0
fi

# Emit "EnumName Variant" for every variant of every #[error_code] enum.
pairs="$(awk '
  /#\[error_code/            { want = 1; next }
  want && /pub enum [A-Za-z0-9_]+/ {
    match($0, /pub enum [A-Za-z0-9_]+/); name = substr($0, RSTART + 9, RLENGTH - 9)
    inside = 1; want = 0; next
  }
  inside && /^[[:space:]]*}/  { inside = 0; next }
  inside && /^[[:space:]]*#/  { next }
  inside && /^[[:space:]]*\/\/[[:space:]]*retired/ { retired = 1; next }
  inside && /^[[:space:]]*\/\// { next }
  inside && /^[[:space:]]*[A-Z][A-Za-z0-9_]*[[:space:]]*(,|=|$)/ {
    if (retired) { retired = 0; next }
    v = $0; sub(/^[[:space:]]*/, "", v); sub(/[^A-Za-z0-9_].*$/, "", v); print name, v
  }
' "${error_files[@]}")"

missing=()
while read -r enum variant; do
  [ -z "${enum:-}" ] && continue
  if [ ${#test_files[@]} -eq 0 ] || ! grep -q "\b${enum}::${variant}\b" "${test_files[@]}"; then
    missing+=("${enum}::${variant}")
  fi
done <<< "$pairs"

total="$(printf '%s\n' "$pairs" | grep -c . || true)"
if [ ${#missing[@]} -gt 0 ]; then
  echo "check-error-coverage: ${#missing[@]} of ${total} error variants have no asserting test:"
  printf '  %s\n' "${missing[@]}"
  exit 1
fi
echo "check-error-coverage: all ${total} error variants are asserted by a test"
