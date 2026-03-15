# Rs Implementation Plan

## Implementation Strategy

Phase 1 first: everything as standard Rust crates. This follows the migration.md plan and lets cyb os development start immediately.

### Repository Structure

```
rs/
├── core/                   rs-lang on crates.io — types, bounded, arena, channel, fixed_point
│   ├── Cargo.toml
│   └── src/
├── macros/                 rs-lang-macros on crates.io — addressed, epoch, deterministic,
│   ├── Cargo.toml                                        registers, cell
│   └── src/
├── tests/                  integration tests
├── reference/              (existing — spec)
├── docs/                   (existing — documentation)
├── Cargo.toml              workspace manifest
├── CLAUDE.md
└── README.md
```

### Dependency Graph

```
rs-lang (library crate, directory: core/)
├── cyber-hemera            (external dep — Hemera hash)
└── (no other external deps)

rs-lang-macros (proc-macro crate, directory: macros/)
├── syn, quote, proc-macro2 (build deps)
└── rs-lang                 (runtime dep — types used in generated code)
```

Two crates. `rs-lang` is the library (types + data structures). `rs-lang-macros` is the single proc-macro crate (all 5 macros). User depends on `rs-lang` which re-exports macros from `rs-lang-macros`.

User-facing API:
```rust
use rs_lang::prelude::*;    // Rust converts hyphens to underscores
```

---

## Phase 1: Library Implementation

### rs-lang (library crate, ~2550 lines, directory: core/)

All library code lives in one crate, organized as modules.

```
core/src/
  lib.rs          — re-exports, prelude (~100 lines)
  core.rs         — Address, Particle, Timeout, traits (~150 lines)
  fixed_point.rs  — FixedPoint<T, DECIMALS> (~400 lines)
  fixed_point/
    ops.rs        — checked/saturating/wrapping ops (~400 lines)
  bounded.rs      — BoundedVec, BoundedMap, ArrayString (~600 lines)
  arena.rs        — Arena<T, N> (~400 lines)
  channel.rs      — bounded MPMC channel (~500 lines)
```

#### core module (~150 lines)

```
  - type Address = [u8; 32]
  - type Particle = hemera::Hash        // re-export from cyber-hemera
  - trait EpochReset { fn reset(&mut self); }
  - trait CanonicalSerialize { fn serialize_canonical(&self, buf: &mut Vec<u8>); }
  - trait Cell { const NAME, VERSION, BUDGET, HEARTBEAT; fn current_epoch(); fn health_check(); fn reset_epoch_state(); }
  - trait MigrateFrom<T> { fn migrate(old: T) -> Self; }
  - struct FunctionSignature { name, args, ret, deadline }
  - trait CellMetadata { fn interface() -> &[FunctionSignature]; }
  - struct Timeout                      // returned when bounded async exceeds deadline
```

External dependency: `cyber-hemera` (crates.io). Otherwise `core`/`no_std` only.

#### fixed_point module (~800 lines)

```
  - struct FixedPoint<T, const DECIMALS: u32> { raw: T }
  - from_integer, from_raw, from_decimal
  - checked_add, checked_sub, checked_mul, checked_div
  - saturating_add, saturating_sub, saturating_mul
  - wrapping_add, wrapping_sub, wrapping_mul
  - Ord, Eq, Display, Debug
  - const ZERO, ONE, MAX
  - Instantiations for T = u64, u128
```

Key: `checked_mul` for FixedPoint requires widening multiplication (u128 * u128 needs u256 or split multiplication). This is the tricky part.

#### bounded module (~600 lines)

```
  bounded_vec.rs  — BoundedVec<T, const N: usize>, ~200 lines
  bounded_map.rs  — BoundedMap<K, V, const N: usize> (backed by sorted array), ~250 lines
  array_string.rs — ArrayString<const N: usize>, ~100 lines
```

`BoundedVec`: stack-allocated array + length counter. `try_push`, `try_insert`, `pop`, `iter`, `len`, `clear`.

`BoundedMap`: sorted `BoundedVec<(K, V), N>` with binary search. `try_insert`, `get`, `remove`, `iter`, `entry`, `contains_key`.

`ArrayString`: `BoundedVec<u8, N>` with UTF-8 invariant.

#### arena module (~400 lines)

```
  - struct Arena<T, const N: usize> { storage: [MaybeUninit<T>; N], count: usize }
  - alloc(&self, value: T) -> Option<&mut T>  (uses UnsafeCell internally)
  - count(), iter(), iter_mut()
  - Drop impl frees all
  - clear() resets count
```

One tricky part: `alloc` takes `&self` but returns `&mut T`. This requires interior mutability (`UnsafeCell` + atomic counter). Well-studied pattern.

#### channel module (~500 lines)

```
  - fn bounded_channel<T>(cap: usize) -> (Sender<T>, Receiver<T>)
  - Sender::try_send(T) -> Result<(), Full<T>>
  - Receiver::try_recv() -> Option<T>
  - Receiver::recv() async -> T  (polls try_recv, respects caller deadline)
  - Based on ring buffer with atomic head/tail
```

**Estimate for rs-lang crate:** 4 sessions.

---

### rs-lang-macros (proc-macro crate, ~4000 lines, directory: macros/)

All proc-macros in one crate. Mirrors serde's pattern (serde + serde_derive).

```
macros/src/
  lib.rs          — proc-macro entry points (~50 lines)
  addressed.rs    — #[derive(Addressed)] (~500 lines)
  epoch.rs        — #[epoch] attribute (~300 lines)
  deterministic.rs — #[deterministic] attribute (~400 lines)
  registers.rs    — #[register] attribute on modules (~800 lines)
  cell.rs         — cell! {} macro (~2000 lines)
```

Dependencies: `syn`, `quote`, `proc-macro2`. Runtime dep: `rs-lang`.

#### addressed (~500 lines)

```
  - Parse struct fields
  - Generate CanonicalSerialize impl:
    - Each field serialized in declaration order
    - Integers: LE fixed-width
    - bool: u8 (0/1)
    - Option<T>: u8 tag + data if Some
    - Variable-length: u32 length prefix + data
    - Nested Addressed: serialize as Particle (64 bytes)
    - Enums: u32 discriminant + variant data
  - Generate fn particle(&self) -> Particle { hemera::hash(self.serialize_canonical()) }
  - Compile-time checks:
    - Reject f32/f64 fields
    - Reject raw pointer fields
    - Reject HashMap fields
    - Reject types without CanonicalSerialize
```

#### epoch (~300 lines)

```
  - On statics: wraps the static in a newtype that tracks epoch
  - On structs: generates EpochReset impl that resets all fields
  - Generates __epoch_reset() function that resets all #[epoch] items in the module
  - Reset rules by type:
    - AtomicU32/U64: store(0)
    - BoundedVec/BoundedMap: clear()
    - Option: None
    - bool: false, integers: 0
    - Custom types: require EpochReset impl
```

#### deterministic (~400 lines)

```
  - Parse function AST
  - Walk the token tree / syn AST looking for:
    - f32/f64 type annotations → error
    - HashMap type usage → error
    - rand:: path segments → error
    - std::time::Instant → error
    - unsafe blocks → error (conservative)
    - inline asm → error
  - Cannot check (Phase 2 compiler-enforced):
    - Transitivity (calling non-deterministic functions)
    - Unchecked arithmetic operators
```

#### registers (~800 lines)

```
  - Parse module with register/reg/field attributes
  - Validate:
    - Field bit ranges don't overlap
    - Fields fit within register width
    - Enum variants fit within field width
    - Enum covers all bit patterns for field width
    - Offset within bank_size
  - Generate:
    - read() for ro/rw registers
    - write(FnOnce(&mut Self)) for wo/rw registers
    - modify(FnOnce(&mut Self)) for rw registers
    - Pack/unpack code with shifts and masks
    - Match-based enum conversion (no transmute)
  - All generated code uses unsafe internally but exposes safe API
```

#### cell (~2000 lines)

```
  - Parse cell declaration syntax:
    - name, version, budget, heartbeat
    - state { } block → generate XxxState struct
    - epoch_state { } block → generate XxxEpochState struct with #[epoch]
    - input/output channel declarations
    - pub fn / fn methods → generate impl block
    - async(dur) fn → wrap with timeout (library-level)
    - migrate from vN { } → generate MigrateFrom impl
  - Generate:
    - State struct + EpochState struct
    - Cell wrapper struct
    - Cell trait impl (including current_epoch)
    - Error enum (collected from Error::Variant usage) with From<rs::Timeout>
    - MigrateFrom impl
    - CellMetadata impl (interface introspection)
    - __epoch_reset glue
  - Validation:
    - Version is u32
    - Budget and heartbeat are valid durations
    - State fields have known types
    - Migration references valid old fields
```

This is the largest and most complex piece. The macro is essentially a DSL parser + code generator.

**Estimate for rs-lang-macros crate:** 8 sessions.

---

### Tests

#### Unit tests (in each module)

Each module has `#[cfg(test)] mod tests` with:
- Property tests for arithmetic (fixed_point)
- Edge cases: 0, 1, MAX, overflow
- Serialization round-trip tests (addressed)
- Bitfield pack/unpack round-trips (registers)
- Channel concurrency tests
- Arena fill-and-drop tests

#### Integration tests (~500 lines)

```
tests/
  tutorial_cyb_cell.rs  — the tutorial example compiles and runs
  all_primitives.rs     — each primitive used independently
  migration.rs          — cell v0 → v1 migration works
  deterministic.rs      — #[deterministic] rejects float usage
  bounded_async.rs      — timeout wrapper works
```

**Estimate for all tests:** 2 sessions.

---

## Phase 1 Summary

| Component | Lines | Sessions |
|-----------|------:|:--------:|
| rs-lang (library: core + bounded + arena + channel + fixed_point) | 2,550 | 4 |
| rs-lang-macros (addressed + epoch + deterministic + registers + cell) | 4,000 | 8 |
| tests | 500 | 2 |
| **Total** | **~7,050** | **~14** |

14 sessions = ~42 focused hours.

**External dependency:** `cyber-hemera` v0.2.0 (crates.io) provides Hemera hash (Poseidon2/Goldilocks sponge). `Particle` is a type alias for `hemera::Hash` (64 bytes).

**Parallelization:** The two crates can be developed in parallel by two agents (one on core/, one on macros/). Within each crate, modules are independent files.

---

## Implementation Order

1. **core: core module** — traits and types everything depends on
2. **core: bounded** — used everywhere (cell state, channels)
3. **core: fixed_point** — used in deterministic functions
4. **core: arena** — standalone
5. **core: channel** — standalone
6. **macros: addressed** — simplest macro, validates the macro setup
7. **macros: epoch** — simple, needed conceptually by cell
8. **macros: deterministic** — simple, partial enforcement
9. **macros: registers** — complex, independent
10. **macros: cell** — largest, uses everything
11. **tests** — integration tests after both crates exist

---

## Phase 2: Compiler Patch (future)

Not part of this implementation round. Documented here for completeness.

**Source:** Fork the official `rust-lang/rust` repo at a stable release. The compiler binary is `rsc`. See `reference/compiler.md` for full architecture.

After Phase 1 is stable and cyb os is running on library implementations:

1. Fork rustc at a stable release
2. Add `rs` edition recognition
3. Parser extension for `async(<duration>)` syntax
4. Lint passes: no-heap, no-dyn, no-panic-unwind, deterministic transitivity, bounded async enforcement
5. Register MMIO codegen (compiler-verified version of registers macro)
6. Full rustc test suite + top 1000 no_std crates CI
7. Rs-specific test suite

Estimated: ~2,500 lines of compiler patches. 4-6 sessions.

---

## Quality Gates

Each module must pass before moving on:

1. `cargo test` — all tests pass
2. `cargo clippy` — zero warnings
3. `#![no_std]` compatible (except test harness)
4. `#![deny(unsafe_code)]` on all non-codegen modules
5. No dependencies beyond `core`, `alloc` (for test only), `syn`/`quote` (macros only)
6. Documentation on all public items
7. File size < 500 lines per file
