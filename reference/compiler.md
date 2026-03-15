---
tags: cyber, rs, reference
---

# Compiler Implementation

## Architecture

```
┌──────────────────────────────────────────────┐
│                    rsc                        │
│        (Rs Compiler Driver)                  │
│                                              │
│  ┌──────────────────────────────────────┐    │
│  │      rustc (vendored, not forked)    │    │
│  │                                      │    │
│  │  ┌────────────┐                      │    │
│  │  │   Parser    │  (unchanged)        │    │
│  │  │            │  no syntax changes   │    │
│  │  └─────┬──────┘                      │    │
│  │        │                             │    │
│  │        ▼                             │    │
│  │  ┌────────────────────────────────┐  │    │
│  │  │          HIR / MIR             │  │    │
│  │  │       (unchanged)              │  │    │
│  │  └─────────────┬──────────────────┘  │    │
│  │                │                     │    │
│  │                ▼                     │    │
│  │  ┌────────────────────────────────┐  │    │
│  │  │     Lint Passes (injected)     │  │    │
│  │  │  ┌──────────────────────────┐  │  │    │
│  │  │  │  Rs Edition Lints        │  │  │    │
│  │  │  │  - no heap (~250 lines)  │  │  │    │
│  │  │  │  - no dyn  (~50 lines)   │  │  │    │
│  │  │  │  - no panic-unwind       │  │  │    │
│  │  │  │    (~50 lines)           │  │  │    │
│  │  │  │  - no nondeterministic   │  │  │    │
│  │  │  │    (~50 lines)           │  │  │    │
│  │  │  │  - deterministic full    │  │  │    │
│  │  │  │    (~350 lines)          │  │  │    │
│  │  │  │  - bounded async check   │  │  │    │
│  │  │  │    (~200 lines)          │  │  │    │
│  │  │  │  - step context          │  │  │    │
│  │  │  │    (~100 lines)          │  │  │    │
│  │  │  │  - addressed verify      │  │  │    │
│  │  │  │    (~150 lines)          │  │  │    │
│  │  │  └──────────────────────────┘  │  │    │
│  │  └─────────────┬──────────────────┘  │    │
│  │                │                     │    │
│  │                ▼                     │    │
│  │  ┌────────────────────────────────┐  │    │
│  │  │    LLVM Backend (unchanged)    │  │    │
│  │  └────────────────────────────────┘  │    │
│  └──────────────────────────────────────┘    │
│                                              │
│  ┌──────────────────────────────────────┐    │
│  │  rs-lang-macros (proc-macro crate)   │    │
│  │  directory: macros/                  │    │
│  │  - #[derive(Addressed)]        500L  │    │
│  │  - #[step]                     300L  │    │
│  │  - #[deterministic]            400L  │    │
│  │  - #[register]                 800L  │    │
│  │  - cell! { }                  2000L  │    │
│  └──────────────────────────────────────┘    │
│                                              │
│  ┌──────────────────────────────────────┐    │
│  │  rs-lang (library crate)             │    │
│  │  directory: core/                    │    │
│  │  - core (+ cyber-hemera)        200L │    │
│  │  - fixed_point                  800L │    │
│  │  - bounded                      650L │    │
│  │  - channel                      500L │    │
│  │  - arena                        400L │    │
│  └──────────────────────────────────────┘    │
│                                              │
└──────────────────────────────────────────────┘
```

## Vendor+Patch Technique

rsc uses the same technique as Trisha (~/git/trisha): fetch upstream source, inject hooks via surgical string replacements, build against vendored code. No fork. No separate repository.

```
rsc/patches/apply.nu:
  1. Fetch rustc source for pinned stable release
  2. Inject rs_edition.rs — add Rs variant to Edition enum
  3. Inject lint passes — register in lint store
  4. Widen pub(crate) → pub on internal types where lints need access
  5. Inject diagnostics — error codes and help text
  6. Result: .vendor/rustc ready to compile
```

Advantages over forking:
- Build in minutes (small binary), not hours (full rustc rebuild)
- New rustc releases: re-run apply.nu against new version
- Single repo: rsc/ lives alongside core/ and macros/
- No upstream tracking burden

## Line Count Breakdown

| Component | Location | Lines | Nature |
|-----------|----------|------:|--------|
| Rs edition lints (8 passes) | rsc/patches/ | 1,200 | Injected lint passes |
| Rs edition recognition | rsc/patches/ | 100 | Injected edition variant |
| Rs diagnostics and error messages | rsc/patches/ | 300 | Injected error codes |
| apply.nu (vendor+patch script) | rsc/patches/ | 400 | Build script |
| **rsc subtotal** | | **2,000** | |
| `rs-lang-macros` (all proc-macros) | macros/ (proc-macro crate) | 4,000 | Standard Rust |
| `rs-lang` (all library code) | core/ (library crate) | 2,550 | Standard Rust |
| `cyber-hemera` (Particle/Hemera) | external dep (crates.io) | — | Standard Rust |
| **Crate subtotal** | | **6,550** | |
| **Total** | | **~8,550** | |

Two standard Rust crates (`rs-lang` + `rs-lang-macros`) provide the library and macros. `rsc` is a compiler driver built via vendor+patch. Hemera (Poseidon2/Goldilocks hash) is an external dependency (`cyber-hemera` on crates.io).

## No Parser Change

The `cell!` macro handles `async(dur)` syntax internally (parses its own token stream). Outside cells, `#[bounded_async(dur)]` attribute macro provides the same functionality — valid Rust syntax. No rustc parser modification needed.

## Build Pipeline

```bash
# Build rsc (one-time setup)
$ cd rs/rsc
$ nu patches/apply.nu          # fetch rustc + inject hooks
$ cargo build --release        # builds rsc binary

# Use rsc
$ rsc my_program.rs                    # standard Rust mode
$ rsc --edition rs my_program.rs       # Rs mode with all checks

# Or via Cargo
$ cargo +rsc build                     # uses rsc as compiler
```

## Dual Enforcement

Proc-macros and compiler lints enforce overlapping rules at different levels:

| Check | Proc-macro (works with rustc) | rsc lint (MIR/HIR level) |
|-------|-------------------------------|-------------------------|
| Deterministic: floats, HashMap, rand | Token-level scan | MIR type analysis |
| Deterministic: transitivity | — | MIR call graph (RS209) |
| Deterministic: unchecked arithmetic | — | MIR operator analysis (RS206) |
| Addressed: type restrictions | Token-level reject | MIR transitivity verify |
| Edition restrictions (RS501-507) | — | HIR type walk |
| Bounded async enforcement | Inside `cell!` only | All async fn (RS101) |
| Step context | Inside `cell!` only | Cross-cell enforcement (RS401) |

Code compiled with standard rustc gets proc-macro enforcement. Code compiled with rsc gets both layers. Same RS error codes in both.

## Compatibility Testing

CI runs three test suites:

1. **Rust test suite**: the full rustc test suite must pass with rsc (zero regressions)
2. **Top 1000 no_std crates**: compile with rsc to verify superset property
3. **Rs-specific tests**: test all 7 primitives, all error codes, all edge cases
