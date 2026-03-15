# Rs Implementation Plan

## Strategy

Three parallel tracks developed simultaneously. No sequential phasing — library, macros, and compiler driver are independent work streams that converge at integration testing. Vendor+patch technique (proven in Trisha) avoids forking rustc.

### Repository Structure

```
rs/
├── core/                   rs-lang on crates.io — types, bounded, arena, channel, fixed_point
│   ├── Cargo.toml
│   └── src/
├── macros/                 rs-lang-macros on crates.io — addressed, step, deterministic,
│   ├── Cargo.toml                                        registers, cell
│   └── src/
├── rsc/                    compiler driver (vendor+patch, not a fork)
│   ├── patches/
│   │   ├── apply.nu        — fetch rustc + inject hooks (à la Trisha)
│   │   ├── rs_edition.rs   — edition recognition
│   │   ├── rs_lints.rs     — lint passes (RS001-RS507)
│   │   └── rs_diag.rs      — error messages
│   ├── .vendor/            — fetched + patched rustc (gitignored)
│   ├── src/
│   │   └── main.rs         — rsc binary entry point
│   └── Cargo.toml
├── tests/                  integration tests
├── reference/              (existing — spec)
├── docs/                   (existing — documentation)
├── Cargo.toml              workspace manifest (core + macros; rsc builds separately)
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

rsc (compiler driver, directory: rsc/)
├── .vendor/rustc           (fetched + patched at build time, not forked)
├── rs-lang                 (path dep for test suite)
└── rs-lang-macros          (path dep for test suite)
```

User-facing API:
```rust
use rs_lang::prelude::*;    // Rust converts hyphens to underscores
```

---

## Track A: Library (core/)

### rs-lang (library crate, ~2550 lines)

All library code lives in one crate, organized as modules.

```
core/src/
  lib.rs          — re-exports, prelude (~100 lines)
  core.rs         — Address, Particle, Timeout, traits (~150 lines)
  fixed_point/
    mod.rs        — struct definition, constructors, constants, re-exports (~200 lines)
    ops.rs        — checked/saturating/wrapping arithmetic (~400 lines)
    fmt.rs        — Display, Debug implementations (~100 lines)
    convert.rs    — From impls, from_integer, from_decimal (~100 lines)
  bounded.rs      — BoundedVec, BoundedMap, ArrayString (~600 lines)
  arena.rs        — Arena<T, N> (~400 lines)
  channel.rs      — bounded MPMC channel (~500 lines)
```

#### core module (~150 lines)

```
  - type Address = [u8; 32]
  - type Particle = hemera::Hash        // re-export from cyber-hemera
  - trait StepReset { fn reset(&mut self); }
  - trait CanonicalSerialize { fn serialize_canonical<W: Write>(&self, w: &mut W); }
  - trait Cell { const NAME, VERSION, BUDGET, HEARTBEAT; fn current_step(); fn health_check(); fn reset_step_state(); }
  - trait MigrateFrom<T> { fn migrate(old: T) -> Self; }
  - struct FunctionSignature { name, args, ret, deadline }
  - trait CellMetadata { fn interface() -> &[FunctionSignature]; }
  - struct Timeout                      // returned when bounded async exceeds deadline
```

`CanonicalSerialize` uses a generic writer (`W: Write`) to avoid heap allocation. Callers pass a `BoundedVec<u8, N>` (which implements `Write`) or a `&mut [u8]` cursor. No `Vec<u8>` dependency.

External dependency: `cyber-hemera` (crates.io). Otherwise `core`/`no_std` only.

Re-export paths:
- `rs_lang::Particle` (from `core.rs`, alias for `hemera::Hash`)
- `rs_lang::particle::Particle` (module re-export for `rs::particle` stdlib compat)
- `rs_lang::Address`

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

`BoundedVec`: fixed-capacity collection, no growth after construction. `try_push`, `try_insert`, `pop`, `iter`, `len`, `clear`.

Storage model: for small N (fits on stack), uses `[MaybeUninit<T>; N]` inline. For large N (cell state maps with millions of entries), the containing struct is heap-allocated once at cell creation — the BoundedVec itself is fixed-capacity and never grows. The "no heap" rule (RS502) targets growable allocations (`Vec::push` can realloc), not fixed-capacity containers allocated once.

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
  - fn bounded_channel<T, const N: usize>() -> (Sender<T, N>, Receiver<T, N>)
  - Sender::try_send(T) -> Result<(), Full<T>>
  - Receiver::try_recv() -> Option<T>
  - Receiver::recv() async -> T  (polls try_recv, respects caller deadline)
  - Based on ring buffer with atomic head/tail
```

Channel capacity is a const generic (`N`), consistent with the compile-time bounds philosophy. The cell runtime connects typed, sized channels between cells at initialization.

**Estimate:** 4 sessions.

---

## Track B: Proc-macros (macros/)

### rs-lang-macros (proc-macro crate, ~4000 lines)

All proc-macros in one crate. Mirrors serde's pattern (serde + serde_derive).

```
macros/src/
  lib.rs          — proc-macro entry points (~50 lines)
  addressed.rs    — #[derive(Addressed)] (~500 lines)
  step.rs         — #[step] attribute (~300 lines)
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

#### step (~300 lines)

```
  - On statics: wraps the static in a newtype that tracks step
  - On structs: generates StepReset impl that resets all fields
  - Generates __step_reset() function that resets all #[step] items in the module
  - Reset rules by type:
    - AtomicU32/U64: store(0)
    - BoundedVec/BoundedMap: clear()
    - Option: None
    - bool: false, integers: 0
    - Custom types: require StepReset impl
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
  - Partial enforcement only — rsc adds:
    - RS206: unchecked arithmetic (MIR-level)
    - RS209: transitivity (MIR call graph)
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
    - step_state { } block → generate XxxStepState struct with #[step]
    - input/output channel declarations
    - pub fn / fn methods → generate impl block
    - async(dur) fn → wrap with timeout (library-level)
    - migrate from vN { } → generate MigrateFrom impl
  - Generate:
    - State struct + StepState struct
    - Cell wrapper struct
    - Cell trait impl (including current_step)
    - Error enum (collected from Error::Variant usage) with From<rs::Timeout>
    - MigrateFrom impl
    - CellMetadata impl (interface introspection)
    - __step_reset glue
  - Validation:
    - Version is u32
    - Budget and heartbeat are valid durations
    - State fields have known types
    - Migration references valid old fields
```

This is the largest and most complex piece. The macro is essentially a DSL parser + code generator.

`async(Duration)` syntax works inside the `cell!` macro (which parses its own token stream). Outside cells, use `#[bounded_async(dur)]` attribute macro — valid Rust syntax, handled by rs-lang-macros. No parser change needed.

**Estimate:** 8 sessions.

---

## Track C: Compiler Driver (rsc/)

### Vendor+Patch Architecture

Same technique as Trisha (~/git/trisha): fetch upstream, inject hooks via surgical string replacements, build against vendored source. No fork, no separate repo.

```
rsc/patches/apply.nu:
  1. Fetch rustc source for pinned stable release from crates.io registry
  2. Inject rs_edition.rs — add Rs variant to Edition enum
  3. Inject rs_lints.rs — register 7 lint passes in lint store
  4. Widen pub(crate) → pub on internal types where lint passes need access
  5. Inject rs_diag.rs — error code definitions and help text
  6. Result: .vendor/rustc ready to compile
```

### Lint Passes (~1200 lines)

All lint passes are regular Rust source files in `rsc/patches/`, injected into rustc at build time.

**rs_no_heap.rs (~200 lines)** — RS501, RS502, RS503, RS505, RS507
```
  - Walk HIR type nodes
  - Flag: Box<T>, Vec<T>, String, Arc<T>, Rc<T>, HashMap<K,V>, HashSet<T>
  - Allow with #[allow(rs::heap)]
  - Allow in #[cfg(test)] blocks
```

**rs_no_dyn.rs (~50 lines)** — RS504
```
  - Walk HIR for dyn Trait types
  - Flag: Box<dyn T>, &dyn T
  - Allow with #[allow(rs::dyn_dispatch)]
```

**rs_no_panic_unwind.rs (~50 lines)** — RS506
```
  - Check crate panic strategy
  - Flag if panic = "unwind" in rs edition
  - Require panic = "abort"
```

**rs_deterministic.rs (~300 lines)** — RS201-RS209 (full enforcement)
```
  - Walk MIR for #[deterministic] functions
  - Additions over proc-macro:
    - RS206: flag unchecked arithmetic operators (+, -, * without checked_/saturating_/wrapping_)
    - RS209: transitivity — every callee must also be #[deterministic] or const fn
  - Uses MIR call graph, not token scanning
```

**rs_bounded_async.rs (~200 lines)** — RS101
```
  - Walk all async fn declarations in rs edition
  - Flag async fn without deadline (must use #[bounded_async(dur)] or be inside cell!)
  - Allow with #[allow(rs::unbounded_async)]
```

**rs_step.rs (~100 lines)** — RS401
```
  - Flag #[step] state accessed outside cell context in rs edition
```

**rs_addressed.rs (~100 lines)** — RS301-RS304
```
  - Verify CanonicalSerialize transitivity at MIR level
  - Catches cases the proc-macro misses (type aliases, indirect usage)
```

### Edition Recognition (~100 lines)

```
  - Add Rs variant to Edition enum in rustc_span
  - Register "rs" as valid edition string
  - Gate all Rs-specific lints behind edition = "rs" check
  - Standard edition (2021, 2024) behavior unchanged
```

### Diagnostics (~300 lines)

```
  - Error code definitions: RS001-RS008, RS101, RS201-RS209, RS301-RS304, RS401, RS501-RS507
  - Long-form explanations for each (rsc --explain RS201)
  - Suggestions: "use checked_add instead of +" for RS206
  - Help notes linking to Rs documentation
```

### Build Pipeline

```bash
# Build rsc (one-time setup)
$ cd rs/rsc
$ nu patches/apply.nu          # fetch rustc + inject hooks
$ cargo build --release        # builds rsc binary

# Use rsc
$ rsc my_program.rs                    # standard Rust mode
$ rsc --edition rs my_program.rs       # Rs mode with all checks
$ cargo +rsc build                     # uses rsc as compiler
```

No `git clone` of rust-lang/rust. No `./x.py build` taking hours. The vendor script fetches what's needed and injects hooks. Build time: minutes.

**Estimate:** 5 sessions.

---

## Parallel Schedule

```
Session:  1    2    3    4    5    6    7    8    9   10   11   12
Track A: [core][bnd][fp ][ar+ch]
Track B:       [addr][stp][det][reg][reg][cel][cel][cel]
Track C: [scaf][lint][dyn][det][diag]
Tests:                                              [int][int]
```

Track A (core/) starts first — core module provides types everything depends on.
Track B (macros/) starts session 2 — needs core types.
Track C (rsc/) starts session 1 — scaffold, then lint passes, then diagnostics. 5 sessions (1-5).
Tests start session 11 — after all three tracks complete.

### Summary

| Track | Component | Lines | Sessions |
|-------|-----------|------:|:--------:|
| A | rs-lang (core + bounded + fixed_point + arena + channel) | 2,550 | 4 |
| B | rs-lang-macros (addressed + step + deterministic + registers + cell) | 4,000 | 8 |
| C | rsc (lint passes + edition + diagnostics + build pipeline) | 2,000 | 5 |
| — | Integration tests | 500 | 2 |
| | **Total** | **~9,050** | **12** (parallel) |

12 sessions = ~36 focused hours. Down from 22 (14 + 8) in the old sequential two-phase model.

**External dependency:** `cyber-hemera` v0.2.0 (crates.io) provides Hemera hash (Poseidon2/Goldilocks sponge). `Particle` is a type alias for `hemera::Hash` (64 bytes).

**Parallelization:** Three tracks, three directory scopes (core/, macros/, rsc/). No file overlap. Matches the parallel agent pattern.

---

## Enforcement Timeline

Enforcement arrives incrementally, not as a big-bang Phase 2:

| Session | What ships |
|---------|-----------|
| 1 | core types (Address, Particle, Timeout, traits) + rsc scaffold |
| 2 | BoundedVec/BoundedMap/ArrayString + rs_no_heap, rs_no_dyn, rs_no_panic_unwind lints + `#[derive(Addressed)]` |
| 3 | FixedPoint + `#[step]` + rs_bounded_async lint |
| 4 | Arena + Channel + rs_deterministic lint (MIR-level) |
| 5 | `#[deterministic]` (proc-macro level) + rs diagnostics |
| 6-7 | `#[register]` |
| 8-10 | `cell!` macro |
| 11-12 | Integration tests, full compatibility verification |

By session 2, developers already get compiler warnings for heap usage, dyn dispatch, and panic-unwind in rs edition code.

---

## Design Decisions

### No rustc fork

Vendor+patch technique (proven in Trisha). Fetch upstream rustc source, inject lint passes via surgical string replacements in `apply.nu`. Benefits:
- No upstream tracking burden
- Build in minutes (small binary), not hours (full rustc)
- Automatic access to new rustc releases — just re-run apply.nu against new version
- Single repo: rsc/ lives alongside core/ and macros/

### No `async(dur)` parser change

The `cell!` macro handles `async(dur)` syntax internally (parses its own token stream). Outside cells, `#[bounded_async(dur)]` attribute macro provides the same functionality — valid Rust syntax, no parser modification needed. This eliminates the only feature that would have required modifying rustc's parser.

### Proc-macro + compiler lint coexistence

The proc-macros (Track B) and compiler lints (Track C) enforce overlapping rules at different levels:
- Proc-macro: token-level checks, works with standard rustc
- Compiler lint: MIR/HIR-level checks, stronger guarantees, requires rsc

Code compiled with standard rustc gets proc-macro enforcement only. Code compiled with rsc gets both layers. The proc-macros use the same RS error codes as the compiler lints — consistent developer experience.

#### Enforcement by primitive

| Primitive | rustc + rs-lang-macros | rsc (compiler driver) |
|-----------|----------------------|----------------------|
| Typed registers | `#[register]` proc-macro: full validation + codegen | Same (proc-macro handles it) |
| Bounded async | Timeout wrapping inside `cell!`; `#[bounded_async(dur)]` outside | RS101: all async fn must have deadline in rs edition |
| Deterministic functions | `#[deterministic]` proc-macro: floats, HashMap, rand, unsafe, asm | + RS206: unchecked arithmetic, RS209: transitivity (MIR-level) |
| Addressed types | `#[derive(Addressed)]`: full serialization + hashing | + RS301-304: MIR-level transitivity verification |
| Step-scoped state | `#[step]` attribute macro: full reset generation | + RS401: cross-cell context enforcement |
| Cell declarations | `cell!` proc-macro: full code generation | Same (proc-macro handles it) |
| Edition restrictions | Conventions only | RS501-507: heap, Vec, String, dyn, Arc/Rc, panic, HashMap/HashSet |

cyb os development starts immediately using standard Rust with Rs libraries. Enforcement tightens incrementally as rsc lint passes land.

### Lint allow-attributes use `rs::` prefix

`#[allow(rs::heap)]`, `#[allow(rs::dyn_dispatch)]`, `#[allow(rs::unbounded_async)]`. Proc-macros recognize and pass through these attributes so code written for standard rustc compiles unchanged with rsc.

### Error codes are stable

RS001-RS507 defined in the spec. Both proc-macros and compiler lints emit the same codes.

### Upstream (future)

Propose individual Rs features as Rust RFCs where appropriate:
- `#[deterministic]` has general value beyond cyb os
- Bounded async could benefit any reliability-critical Rust code
- Typed registers would benefit the entire embedded Rust ecosystem

Features that are too domain-specific remain in rsc.

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
