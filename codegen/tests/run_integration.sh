#!/usr/bin/env bash
# Phase 1 integration tests: exit42 (exit code) and hello_world (stdout).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CODEGEN_DIR="$(dirname "$SCRIPT_DIR")"
NIGHTLY=~/.rustup/toolchains/nightly-2025-11-26-aarch64-apple-darwin/bin
NIGHTLY_RUSTC="$NIGHTLY/rustc"
NIGHTLY_CARGO="$NIGHTLY/cargo"
OUT_DIR="$(mktemp -d)"
trap 'rm -rf "$OUT_DIR"' EXIT

echo "==> Building codegen dylib..."
cd "$CODEGEN_DIR"
RUSTC="$NIGHTLY_RUSTC" "$NIGHTLY_CARGO" build 2>&1

DYLIB="$CODEGEN_DIR/target/debug/libcodegen.dylib"
if [ ! -f "$DYLIB" ]; then
    echo "FAIL: dylib not found at $DYLIB"
    exit 1
fi
echo "    dylib: $DYLIB"

# ── Test 1: exit42 ────────────────────────────────────────────────
echo "==> Compiling exit42.rs via trident backend..."
BINARY="$OUT_DIR/exit42"
"$NIGHTLY_RUSTC" \
    -Z codegen-backend="$DYLIB" \
    --edition 2021 \
    --target aarch64-apple-darwin \
    -C panic=abort \
    -o "$BINARY" \
    "$SCRIPT_DIR/exit42.rs" 2>&1

if [ ! -f "$BINARY" ]; then
    echo "FAIL: exit42 binary not produced"
    exit 1
fi
echo "    binary: $BINARY ($(wc -c < "$BINARY") bytes)"

echo "==> Running exit42..."
"$BINARY" || CODE=$?
CODE=${CODE:-0}
echo "    exit code: $CODE"

if [ "$CODE" -eq 42 ]; then
    echo "PASS: exit42 — exit code is 42"
else
    echo "FAIL: exit42 — expected 42, got $CODE"
    exit 1
fi

# ── Test 2: hello_world ───────────────────────────────────────────
echo "==> Compiling hello_world.rs via trident backend..."
HW_BINARY="$OUT_DIR/hello_world"
"$NIGHTLY_RUSTC" \
    -Z codegen-backend="$DYLIB" \
    --edition 2021 \
    --target aarch64-apple-darwin \
    -C panic=abort \
    -o "$HW_BINARY" \
    "$SCRIPT_DIR/hello_world.rs" 2>&1

if [ ! -f "$HW_BINARY" ]; then
    echo "FAIL: hello_world binary not produced"
    exit 1
fi
echo "    binary: $HW_BINARY ($(wc -c < "$HW_BINARY") bytes)"

echo "==> Running hello_world..."
HW_CODE=0
"$HW_BINARY" > "$OUT_DIR/hw_out" || HW_CODE=$?
echo "    exit code: $HW_CODE"

if diff -q "$OUT_DIR/hw_out" <(printf 'Hello, world!\n') > /dev/null 2>&1 && [ "$HW_CODE" -eq 0 ]; then
    echo "PASS: hello_world — stdout matches and exit code is 0"
else
    echo "FAIL: hello_world"
    echo "    expected stdout: 'Hello, world!\\n'"
    echo "    actual stdout:   '$(cat "$OUT_DIR/hw_out")'"
    echo "    exit code:       $HW_CODE"
    exit 1
fi

# ── Helper ────────────────────────────────────────────────────────────
run_exit_test() {
    local name="$1" src="$2" expected="$3"
    echo "==> Compiling $name..."
    local bin="$OUT_DIR/$name"
    "$NIGHTLY_RUSTC" \
        -Z codegen-backend="$DYLIB" \
        --edition 2021 \
        --target aarch64-apple-darwin \
        -C panic=abort \
        -o "$bin" \
        "$SCRIPT_DIR/$src" 2>&1
    if [ ! -f "$bin" ]; then echo "FAIL: $name — binary not produced"; exit 1; fi
    local code=0
    "$bin" || code=$?
    if [ "$code" -eq "$expected" ]; then
        echo "PASS: $name — exit code $code"
    else
        echo "FAIL: $name — expected $expected, got $code"; exit 1
    fi
}

# ── Test 3: struct_ops ────────────────────────────────────────────────
run_exit_test struct_ops struct_ops.rs 42

# ── Test 4: fnptr ─────────────────────────────────────────────────────
run_exit_test fnptr fnptr.rs 7

# ── Test 5: cast_ops ──────────────────────────────────────────────────
run_exit_test cast_ops cast_ops.rs 42
