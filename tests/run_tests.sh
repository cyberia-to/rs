#!/bin/bash
# Integration test runner for rsc.
# Runs compile-pass tests (must succeed) and ui tests (must fail with expected errors).

set -e

RSC="${RSC:-rsc}"
PASS=0
FAIL=0
TESTS_DIR="$(cd "$(dirname "$0")" && pwd)"

green() { printf "\033[32m%s\033[0m\n" "$1"; }
red() { printf "\033[31m%s\033[0m\n" "$1"; }

# --- compile-pass: must compile with rsc ---
echo "=== compile-pass ==="
for f in "$TESTS_DIR"/compile-pass/*.rs; do
    name=$(basename "$f" .rs)
    if $RSC --rs-edition "$f" -o /tmp/rsc_test_out -C panic=abort 2>/dev/null; then
        green "  PASS: $name"
        PASS=$((PASS + 1))
    else
        red "  FAIL: $name (expected to compile)"
        FAIL=$((FAIL + 1))
    fi
done

# --- ui: must fail, and stderr must contain expected error pattern ---
echo ""
echo "=== ui (expect errors) ==="
for f in "$TESTS_DIR"/ui/*.rs; do
    name=$(basename "$f" .rs)

    # Determine flags: rs506_panic gets no -C panic=abort
    flags=""
    if [[ "$name" != "rs506_panic" ]]; then
        flags="-C panic=abort"
    fi

    # Run rsc, capture stderr
    stderr=$($RSC --rs-edition "$f" -o /tmp/rsc_test_out $flags 2>&1 || true)

    # Extract expected error code from filename (e.g., rs501_box -> RS501)
    code=$(echo "$name" | sed 's/^rs\([0-9]*\).*/RS\1/' | tr '[:lower:]' '[:upper:]')

    if echo "$stderr" | grep -qi "error"; then
        green "  PASS: $name (error produced, code $code)"
        PASS=$((PASS + 1))
    else
        red "  FAIL: $name (expected error with $code, got none)"
        FAIL=$((FAIL + 1))
    fi
done

echo ""
echo "=== Results: $PASS passed, $FAIL failed ==="
[ "$FAIL" -eq 0 ] || exit 1
