---
tags: cyber, rs, reference
---

# Deterministic Functions

## Problem

Blockchain consensus requires that every node produces identical output for identical input. Rust does not guarantee this. Floating point operations can produce different results on different architectures. Integer overflow behavior depends on debug/release mode. Memory layout of structs is unspecified.

## Solution

The `#[deterministic]` attribute marks functions that must produce identical results on all platforms.

## Syntax

```rust
#[deterministic]
fn compute_rank(weights: &[FixedPoint<u128, 18>]) -> Option<FixedPoint<u128, 18>> {
    let mut sum = FixedPoint::ZERO;
    for w in weights {
        sum = sum.checked_add(*w)?;  // checked arithmetic required
    }
    Some(sum)
}
```

## Compile-Time Checks

Inside a `#[deterministic]` function, the compiler rejects:

| Rejected Construct | Reason | Error Code |
|---|---|---|
| `f32`, `f64` types | Non-deterministic across platforms | RS201 |
| `as` casts involving floats | Rounding is platform-dependent | RS202 |
| Raw pointer arithmetic | Addresses are non-deterministic | RS203 |
| `std::time::Instant` | Wall clock is non-deterministic | RS204 |
| `rand::*` | Randomness is non-deterministic | RS205 |
| Unchecked arithmetic (`+`, `-`, `*`) | Overflow behavior differs debug/release | RS206 |
| `HashMap` iteration | Order is non-deterministic | RS207 |
| Inline assembly | Platform-specific by definition | RS208 |
| Calling non-`#[deterministic]` functions | Transitivity requirement | RS209 |

## What IS Allowed

- `FixedPoint<T, DECIMALS>` (Rs built-in fixed-point type)
- `checked_add`, `checked_mul`, `checked_sub`, `checked_div`
- `saturating_*` arithmetic
- `wrapping_*` arithmetic (deterministic across platforms)
- `overflowing_*` arithmetic (returns value + overflow flag)
- `BTreeMap`, `BTreeSet` (deterministic iteration order)
- Arrays, slices with deterministic indexing
- Other `#[deterministic]` functions
- `const fn` (already deterministic)
- All comparison and logical operations

## Transitivity

Determinism is contagious upward and required downward:

```rust
#[deterministic]
fn outer() -> u64 {
    inner()  // OK only if inner() is also #[deterministic]
}

#[deterministic]
fn inner() -> u64 {
    42
}

fn non_det() -> u64 {
    outer()  // OK: non-deterministic can call deterministic
}
```

## Built-in FixedPoint Type

Rs provides a built-in fixed-point numeric type:

```rust
// FixedPoint<BaseType, DecimalPlaces>
type Rank = FixedPoint<u128, 18>;   // 18 decimal places, u128 backing

let a: Rank = Rank::from_integer(42);
let b: Rank = Rank::from_raw(42_000_000_000_000_000_000u128); // 42.0
let c = a.checked_add(b).unwrap();
let d = a.checked_mul(b).unwrap();

// All operations are deterministic, checked, no floats
```

Compiler implementation: ~400 lines (lint pass + diagnostics).
