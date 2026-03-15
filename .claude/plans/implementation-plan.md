# Rs Implementation Plan

## Implementation Strategy

Phase 1 first: everything as standard Rust crates. This follows the migration.md plan and lets cyb os development start immediately.

### Repository Structure

```
rs/
├── reference/              (existing — spec)
├── docs/                   (existing — documentation)
├── crates/
│   ├── rs-core/            types shared across all crates (depends on cyber-hemera)
│   ├── rs-fixed-point/     FixedPoint<T, DECIMALS>
│   ├── rs-bounded/         BoundedVec, BoundedMap, ArrayString
│   ├── rs-arena/           Arena<T, N>
│   ├── rs-channel/         bounded MPMC channel
│   ├── rs-addressed/       #[derive(Addressed)] proc-macro
│   ├── rs-epoch/           #[epoch] attribute macro
│   ├── rs-deterministic/   #[deterministic] proc-macro (partial)
│   ├── rs-registers/       #[register] proc-macro
│   ├── rs-cell/            cell! {} proc-macro
│   └── rs/                 facade crate (rs::prelude::*)
├── tests/                  integration tests
├── Cargo.toml              workspace manifest
├── CLAUDE.md
└── README.md
```

### Dependency Graph

```
rs (facade)
├── rs-core           ← cyber-hemera (external dep)
├── rs-fixed-point    ← rs-core
├── rs-bounded        ← rs-core
├── rs-arena          ← rs-core
├── rs-channel        ← rs-core, rs-bounded
├── rs-addressed      ← rs-core (proc-macro, uses cyber-hemera via rs-core)
├── rs-epoch          ← rs-core (proc-macro)
├── rs-deterministic  ← rs-core (proc-macro)
├── rs-registers      ← rs-core (proc-macro)
└── rs-cell           ← rs-core, rs-epoch, rs-bounded (proc-macro)
```

---

## Phase 1: Library Implementation

### Layer 0: Foundation (no dependencies between these)

#### 0.1 — rs-core (~100 lines)

Shared types and traits used across all crates.

```
src/lib.rs
  - type Address = [u8; 32]
  - trait EpochReset { fn reset(&mut self); }
  - trait CanonicalSerialize { fn serialize_canonical(&self, buf: &mut Vec<u8>); }
  - type Particle = hemera::Hash        // re-export from cyber-hemera
  - trait Cell { const NAME, VERSION, BUDGET, HEARTBEAT; fn current_epoch(); fn health_check(); fn reset_epoch_state(); }
  - trait MigrateFrom<T> { fn migrate(old: T) -> Self; }
  - struct FunctionSignature { name, args, ret, deadline }
  - trait CellMetadata { fn interface() -> &[FunctionSignature]; }
  - struct Timeout                      // returned when bounded async exceeds deadline
```

No dependencies beyond `core`/`no_std`.

**Estimate:** 1 pomodoro.

#### 0.2 — rs-fixed-point (~800 lines)

Deterministic fixed-point arithmetic.

```
src/
  lib.rs
  - struct FixedPoint<T, const DECIMALS: u32> { raw: T }
  - from_integer, from_raw, from_decimal
  - checked_add, checked_sub, checked_mul, checked_div
  - saturating_add, saturating_sub, saturating_mul
  - wrapping_add, wrapping_sub, wrapping_mul
  - Ord, Eq, Display, Debug
  - const ZERO, ONE, MAX
  - Instantiations for T = u64, u128
```

Dependencies: `rs-core`.

Key: `checked_mul` for FixedPoint requires widening multiplication (u128 * u128 needs u256 or split multiplication). This is the tricky part.

**Estimate:** 1 session (6 pomodoros).

#### 0.3 — rs-bounded (~600 lines)

Bounded collections for no-heap environments.

```
src/
  bounded_vec.rs  — BoundedVec<T, const N: usize>, ~200 lines
  bounded_map.rs  — BoundedMap<K, V, const N: usize> (backed by sorted array or BTreeMap), ~250 lines
  array_string.rs — ArrayString<const N: usize>, ~100 lines
  lib.rs          — re-exports, ~50 lines
```

`BoundedVec`: stack-allocated array + length counter. `try_push`, `try_insert`, `pop`, `iter`, `len`, `clear`.

`BoundedMap`: sorted `BoundedVec<(K, V), N>` with binary search. `try_insert`, `get`, `remove`, `iter`, `entry`, `contains_key`.

`ArrayString`: `BoundedVec<u8, N>` with UTF-8 invariant.

Dependencies: `rs-core`.

**Estimate:** 1 session.

#### 0.4 — rs-arena (~400 lines)

Typed arena with compile-time capacity.

```
src/lib.rs
  - struct Arena<T, const N: usize> { storage: [MaybeUninit<T>; N], count: usize }
  - alloc(&self, value: T) -> Option<&mut T>  (uses UnsafeCell internally)
  - count(), iter(), iter_mut()
  - Drop impl frees all
  - clear() resets count
```

One tricky part: `alloc` takes `&self` but returns `&mut T`. This requires interior mutability (`UnsafeCell` + atomic counter). Well-studied pattern.

Dependencies: `rs-core`.

**Estimate:** 1 session.

#### 0.5 — rs-channel (~500 lines)

Wait-free bounded MPMC channel.

```
src/lib.rs
  - fn bounded_channel<T>(cap: usize) -> (Sender<T>, Receiver<T>)
  - Sender::try_send(T) -> Result<(), Full<T>>
  - Receiver::try_recv() -> Option<T>
  - Receiver::recv() async -> T  (polls try_recv, respects caller deadline)
  - Based on ring buffer with atomic head/tail
```

Dependencies: `rs-core`, `rs-bounded` (for internal buffer).

**Estimate:** 1 session.

### Layer 1: Proc-Macros (depend on Layer 0)

#### 1.1 — rs-addressed (~500 lines)

Proc-macro crate: `#[derive(Addressed)]`.

```
src/lib.rs (proc-macro = true)
  - Parse struct fields
  - Generate CanonicalSerialize impl:
    - Each field serialized in declaration order
    - Integers: LE fixed-width
    - Variable-length: u32 length prefix + data
    - Nested Addressed: serialize as Particle (64 bytes)
    - Enums: u32 discriminant + variant data
  - Generate fn particle(&self) -> Particle { Hemera::hash(self.serialize_canonical()) }
  - Compile-time checks:
    - Reject f32/f64 fields
    - Reject raw pointer fields
    - Reject HashMap fields
    - Reject types without CanonicalSerialize
```

Dependencies: `syn`, `quote`, `proc-macro2`. Runtime dep: `rs-core` (which re-exports `cyber-hemera`).

**Estimate:** 1 session.

#### 1.2 — rs-epoch (~300 lines)

Attribute proc-macro: `#[epoch]`.

```
src/lib.rs (proc-macro = true)
  - On statics: wraps the static in a newtype that tracks epoch
  - Generates EpochReset impl for the inner type
  - Generates __epoch_reset() function that resets all #[epoch] statics in the module
  - For known types:
    - AtomicU32/U64: store(0)
    - BoundedVec: clear()
    - Option: None
    - Custom types: require EpochReset impl
```

Dependencies: `syn`, `quote`, `proc-macro2`. Runtime dep: `rs-core`.

**Estimate:** 1 session.

#### 1.3 — rs-deterministic (~400 lines)

Proc-macro that checks function body for non-deterministic constructs (partial — full enforcement requires compiler).

```
src/lib.rs (proc-macro = true)
  - Parse function AST
  - Walk the token tree / syn AST looking for:
    - f32/f64 type annotations → error
    - HashMap type usage → error
    - rand:: path segments → error
    - std::time::Instant → error
    - unsafe blocks → error (conservative)
    - inline asm → error
  - Cannot check:
    - Transitivity (calling non-deterministic functions) — needs type system
    - Unchecked arithmetic operators — looks like normal operators in AST
  - These unchecked items become compiler-enforced in Phase 2
```

**Estimate:** 1 session.

#### 1.4 — rs-registers (~800 lines)

Proc-macro: `#[register]` attribute on modules.

```
src/lib.rs (proc-macro = true)
  - Parse module with register/reg/field attributes
  - Validate:
    - Field bit ranges don't overlap
    - Fields fit within register width
    - Enum variants fit within field width
    - Offset within bank_size
  - Generate:
    - read() for ro/rw registers
    - write(FnOnce(&mut Self)) for wo/rw registers
    - modify(FnOnce(&mut Self)) for rw registers
    - Pack/unpack code with shifts and masks
    - Match-based enum conversion (no transmute)
  - All generated code uses unsafe internally but exposes safe API
```

Dependencies: `syn`, `quote`, `proc-macro2`.

**Estimate:** 2 sessions. This is one of the more complex macros — bitfield codegen, validation, multiple access modes.

#### 1.5 — rs-cell (~2000 lines)

Proc-macro: `cell! {}` declarative macro.

```
src/lib.rs (proc-macro = true)
  - Parse cell declaration syntax:
    - name, version, budget, heartbeat
    - state { } block → generate XxxState struct
    - epoch_state { } block → generate XxxEpochState struct with #[epoch]
    - pub fn / fn methods → generate impl block
    - async(dur) fn → wrap with timeout (library-level)
    - migrate from vN { } → generate MigrateFrom impl
  - Generate:
    - State struct + EpochState struct
    - Cell wrapper struct
    - Cell trait impl
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

Dependencies: `syn`, `quote`, `proc-macro2`. Runtime deps: `rs-core`, `rs-epoch`, `rs-bounded`.

**Estimate:** 3 sessions.

### Layer 2: Facade

#### 2.1 — rs (facade crate, ~100 lines)

```
src/lib.rs
  pub use rs_core::*;
  pub use rs_fixed_point as fixed_point;
  pub use rs_bounded as bounded;
  pub use rs_arena as arena;
  pub use rs_channel as channel;

  pub mod prelude {
      pub use rs_core::{Address, EpochReset, CanonicalSerialize, Cell, ...};
      pub use rs_core::Particle;
      pub use rs_fixed_point::FixedPoint;
      pub use rs_bounded::{BoundedVec, BoundedMap, ArrayString};
      pub use rs_arena::Arena;
      pub use rs_addressed::Addressed;
      pub use rs_epoch::epoch;
      pub use rs_deterministic::deterministic;
      pub use rs_registers::register;
      pub use rs_cell::cell;
  }
```

**Estimate:** 1 pomodoro.

### Layer 3: Tests

#### 3.1 — Unit tests (in each crate)

Each crate has `#[cfg(test)] mod tests` with:
- Property tests for arithmetic (fixed_point)
- Edge cases: 0, 1, MAX, overflow
- Serialization round-trip tests (addressed)
- Bitfield pack/unpack round-trips (registers)
- Channel concurrency tests
- Arena fill-and-drop tests

#### 3.2 — Integration tests (~500 lines)

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

| Component | Lines | Sessions | Layer |
|-----------|------:|:--------:|:-----:|
| rs-core (+Particle via cyber-hemera) | 150 | 0.5 | 0 |
| rs-fixed-point | 800 | 1 | 0 |
| rs-bounded | 600 | 1 | 0 |
| rs-arena | 400 | 1 | 0 |
| rs-channel | 500 | 1 | 0 |
| rs-addressed | 500 | 1 | 1 |
| rs-epoch | 300 | 1 | 1 |
| rs-deterministic | 400 | 1 | 1 |
| rs-registers | 800 | 2 | 1 |
| rs-cell | 2000 | 3 | 1 |
| rs (facade) | 100 | 0.5 | 2 |
| tests | 500 | 2 | 3 |
| **Total** | **~7,050** | **~16** | |

16 sessions = ~48 focused hours. With parallel work on Layer 0 crates, effective time compresses.

**External dependency:** `cyber-hemera` v0.2.0 (crates.io) provides Hemera hash (Poseidon2/Goldilocks sponge). `Particle` is a type alias for `hemera::Hash` (64 bytes). No need to reimplement.

**Parallelization:** Layer 0 crates (0.1–0.5) have no dependencies on each other. All five can be developed in parallel by separate agents, partitioned by crate directory. Layer 1 macros depend on Layer 0 but are independent of each other except rs-cell depends on rs-epoch. Layer 2 and 3 are sequential.

---

## Implementation Order (Sequential Path)

If working sequentially, priority order:

1. **rs-core** — everything depends on it (includes Particle via cyber-hemera)
2. **rs-bounded** — used everywhere (cell state, channels)
3. **rs-fixed-point** — used in deterministic functions
4. **rs-arena** — standalone, simple
5. **rs-channel** — standalone
6. **rs-addressed** — first macro, uses cyber-hemera via rs-core
7. **rs-epoch** — simple macro, needed by rs-cell
8. **rs-registers** — complex macro, independent
9. **rs-deterministic** — simple macro, partial enforcement
10. **rs-cell** — largest macro, uses everything
11. **rs (facade)** — trivial, last
12. **tests** — integration tests after all crates exist

---

## Phase 2: Compiler Patch (future)

Not part of this implementation round. Documented here for completeness.

After Phase 1 is stable and cyb os is running on library implementations:

1. Fork rustc at a stable release
2. Add `rs` edition recognition
3. Parser extension for `async(<duration>)` syntax
4. Lint passes: no-heap, no-dyn, no-panic-unwind, deterministic transitivity, bounded async enforcement
5. Register MMIO codegen (compiler-verified version of rs-registers macro)
6. Full rustc test suite + top 1000 no_std crates CI
8. Rs-specific test suite

Estimated: ~2,500 lines of compiler patches. 4-6 sessions.

---

## Quality Gates

Each crate must pass before moving to the next layer:

1. `cargo test` — all tests pass
2. `cargo clippy` — zero warnings
3. `#![no_std]` compatible (except test harness)
4. `#![deny(unsafe_code)]` on all non-codegen modules
5. No dependencies beyond `core`, `alloc` (for test only), `syn`/`quote` (macros only)
6. Documentation on all public items
7. File size < 500 lines per file
