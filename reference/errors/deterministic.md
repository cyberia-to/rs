---
tags: cyber, rs, reference
---

# Deterministic Errors (RS201–RS210)

[Back to Error Catalog](../errors.md) | Spec: [deterministic.md](../deterministic.md)

Enforcement: proc-macro (RS201–RS205, RS207, RS208) + rsc lint (RS206, RS209, RS210).

---

### RS201: Floating point types

```text
error[RS201]: f64 type used in #[deterministic] function
  help: use FixedPoint<u128, 18> for deterministic decimal arithmetic
```

`f32` and `f64` produce different results on different architectures (x87 vs SSE, ARM vs x86). Forbidden in deterministic functions.

#### Fix

```rust
use rs_lang::fixed_point::FixedPoint;
type Amount = FixedPoint<u128, 18>;

#[deterministic]
fn compute(a: Amount, b: Amount) -> Option<Amount> {
    a.checked_add(b)
}
```

---

### RS202: Float casts

```text
error[RS202]: cast to f64 in #[deterministic] function
  help: rounding behavior of float casts is platform-dependent
```

`as` casts involving floating point types have platform-dependent rounding behavior.

#### Fix

Use integer arithmetic or FixedPoint conversions.

---

### RS203: Raw pointer arithmetic

```text
error[RS203]: raw pointer arithmetic in #[deterministic] function
  help: memory addresses are non-deterministic across runs
```

Pointer values depend on memory layout, ASLR, and allocator state — non-deterministic across different machines or runs.

#### Fix

Use index-based access or references instead of raw pointers.

---

### RS204: System clock

```text
error[RS204]: std::time::Instant used in #[deterministic] function
  help: wall clock time is non-deterministic; use step counters
```

Wall clock time varies between machines and runs.

#### Fix

Use step counters from the cell context (`self.current_step()`).

---

### RS205: Randomness

```text
error[RS205]: rand::thread_rng used in #[deterministic] function
  help: randomness is non-deterministic by definition
```

Random number generators produce different sequences on different runs.

#### Fix

Use deterministic seed-based computation or remove randomness.

---

### RS206: Unchecked arithmetic

```text
error[RS206]: unchecked addition in #[deterministic] function
  help: use checked_add instead of +
  help: overflow behavior differs between debug (panic) and release (wrap)
```

The `+`, `-`, `*` operators on integers have different overflow behavior in debug mode (panic) vs release mode (wrapping). This is a source of non-determinism between build configurations.

Enforcement: **rsc lint only** (MIR-level operator analysis). The proc-macro cannot reliably detect operators in the token stream.

#### Fix

```rust
#[deterministic]
fn add(a: u64, b: u64) -> Option<u64> {
    a.checked_add(b)        // returns None on overflow
    // or: a.saturating_add(b)  // clamps to MAX
    // or: a.wrapping_add(b)    // wraps (deterministic)
}
```

---

### RS207: HashMap iteration

```text
error[RS207]: HashMap used in #[deterministic] function
  help: HashMap iteration order is non-deterministic; use BTreeMap
```

HashMap uses a randomized hasher — iteration order varies between runs and platforms.

#### Fix

Use `BTreeMap` or `BTreeSet` (deterministic iteration order).

---

### RS208: Inline assembly

```text
error[RS208]: inline assembly in #[deterministic] function
  help: assembly is platform-specific by definition
```

Inline assembly (`asm!`, `global_asm!`) produces platform-specific behavior.

#### Fix

Use portable Rust code.

---

### RS209: Non-deterministic callee

```text
error[RS209]: call to non-deterministic function foo() in #[deterministic] function
  help: mark foo() as #[deterministic] or const fn
```

A `#[deterministic]` function calls a function that is neither `#[deterministic]` nor `const fn`. Determinism is transitive — every callee in the call graph must also be deterministic.

Enforcement: **rsc lint only** (MIR call graph analysis). The proc-macro cannot resolve function definitions across modules.

#### Fix

Mark the callee `#[deterministic]`, make it `const fn`, or restructure to avoid the call.

---

### RS210: Platform-dependent integer types

```text
error[RS210]: usize used in #[deterministic] function
  help: usize is 32 bits on 32-bit platforms and 64 bits on 64-bit platforms; use u32 or u64
```

`usize` and `isize` have platform-dependent width. A function that operates on `usize` values may produce different results (overflow, truncation) on 32-bit vs 64-bit targets.

Enforcement: **rsc lint only** (HIR type analysis). The proc-macro cannot reliably distinguish `usize` from other integer types in all contexts.

#### Fix

```rust
#[deterministic]
fn index_value(data: &[u8], idx: u32) -> Option<u8> {
    data.get(idx as usize).copied()  // usize used only for slice indexing, not computation
}
```

Use `u32` or `u64` for values that participate in arithmetic or are serialized. `usize` is permitted only as a transient cast for slice/array indexing where the value originates from a fixed-width type.
