#!/usr/bin/env bash
# Phase 2 integration tests for macho-linker.
# Requires: clang (system), macho-linker binary.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CRATE_DIR="$(dirname "$SCRIPT_DIR")"
WORKSPACE_DIR="$(dirname "$CRATE_DIR")"
OUT_DIR="$(mktemp -d)"
trap 'rm -rf "$OUT_DIR"' EXIT

LINKER="$WORKSPACE_DIR/target/debug/macho-linker"

echo "==> Building macho-linker..."
cd "$WORKSPACE_DIR"
cargo build -p macho-linker 2>&1
if [ ! -f "$LINKER" ]; then
    echo "FAIL: macho-linker binary not found"
    exit 1
fi
echo "    linker: $LINKER"

# ── Test 1: exit42 (no external refs, pure syscall) ────────────────────────
echo "==> Assembling exit42.s..."
EXIT42_OBJ="$OUT_DIR/exit42.o"
clang -arch arm64 -target aarch64-apple-macos15.0 -c \
    "$SCRIPT_DIR/exit42.s" -o "$EXIT42_OBJ"

echo "==> Linking exit42..."
EXIT42_BIN="$OUT_DIR/exit42"
"$LINKER" -o "$EXIT42_BIN" "$EXIT42_OBJ"

if [ ! -f "$EXIT42_BIN" ]; then
    echo "FAIL: exit42 binary not produced"
    exit 1
fi
echo "    binary: $EXIT42_BIN ($(wc -c < "$EXIT42_BIN") bytes)"

echo "==> Running exit42..."
EXIT42_CODE=0
"$EXIT42_BIN" || EXIT42_CODE=$?
echo "    exit code: $EXIT42_CODE"

if [ "$EXIT42_CODE" -eq 42 ]; then
    echo "PASS: exit42 — exit code is 42"
else
    echo "FAIL: exit42 — expected 42, got $EXIT42_CODE"
    exit 1
fi

# ── Test 2: hello (write + exit via libSystem dyld binding) ────────────────
echo "==> Assembling hello.s..."
HELLO_OBJ="$OUT_DIR/hello.o"
clang -arch arm64 -target aarch64-apple-macos15.0 -c \
    "$SCRIPT_DIR/hello.s" -o "$HELLO_OBJ"

echo "==> Linking hello..."
HELLO_BIN="$OUT_DIR/hello"
"$LINKER" -o "$HELLO_BIN" "$HELLO_OBJ" -lSystem

if [ ! -f "$HELLO_BIN" ]; then
    echo "FAIL: hello binary not produced"
    exit 1
fi
echo "    binary: $HELLO_BIN ($(wc -c < "$HELLO_BIN") bytes)"

echo "==> Running hello..."
HELLO_CODE=0
"$HELLO_BIN" > "$OUT_DIR/hello_out" || HELLO_CODE=$?
echo "    exit code: $HELLO_CODE"

if diff -q "$OUT_DIR/hello_out" <(printf 'Hello, world!\n') > /dev/null 2>&1 \
        && [ "$HELLO_CODE" -eq 0 ]; then
    echo "PASS: hello — stdout matches and exit code is 0"
else
    echo "FAIL: hello"
    echo "    expected: 'Hello, world!\\n'"
    echo "    actual:   '$(cat "$OUT_DIR/hello_out")'"
    echo "    exit code: $HELLO_CODE"
    exit 1
fi
