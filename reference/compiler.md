---
tags: cyber, rs, reference
---

# Compiler Implementation

## Architecture

```
┌──────────────────────────────────────────────┐
│                    rsc                        │
│            (Rs Compiler)                      │
│                                              │
│  ┌──────────────────────────────────────┐    │
│  │           rustc (forked)              │    │
│  │                                      │    │
│  │  ┌────────────┐  ┌───────────────┐   │    │
│  │  │   Parser    │  │  Rs Parser    │   │    │
│  │  │  (unchanged)│  │  Extension    │   │    │
│  │  │            │  │  async(dur)   │   │    │
│  │  │            │  │  ~200 lines   │   │    │
│  │  └─────┬──────┘  └──────┬────────┘   │    │
│  │        │                │             │    │
│  │        ▼                ▼             │    │
│  │  ┌────────────────────────────────┐   │    │
│  │  │          HIR / MIR             │   │    │
│  │  │       (unchanged)              │   │    │
│  │  └─────────────┬──────────────────┘   │    │
│  │                │                      │    │
│  │                ▼                      │    │
│  │  ┌────────────────────────────────┐   │    │
│  │  │        Lint Passes             │   │    │
│  │  │  ┌──────────────────────────┐  │   │    │
│  │  │  │  Rs Edition Lints        │  │   │    │
│  │  │  │  - no heap (~200 lines)  │  │   │    │
│  │  │  │  - no dyn  (~50 lines)   │  │   │    │
│  │  │  │  - no panic-unwind       │  │   │    │
│  │  │  │    (~50 lines)           │  │   │    │
│  │  │  │  - no float in det       │  │   │    │
│  │  │  │    (~300 lines)          │  │   │    │
│  │  │  │  - bounded async check   │  │   │    │
│  │  │  │    (~200 lines)          │  │   │    │
│  │  │  └──────────────────────────┘  │   │    │
│  │  └─────────────┬──────────────────┘   │    │
│  │                │                      │    │
│  │                ▼                      │    │
│  │  ┌────────────────────────────────┐   │    │
│  │  │        Codegen                 │   │    │
│  │  │  ┌──────────────────────────┐  │   │    │
│  │  │  │  Register MMIO codegen   │  │   │    │
│  │  │  │  (~800 lines)            │  │   │    │
│  │  │  │  Bounded async desugar   │  │   │    │
│  │  │  │  (~300 lines)            │  │   │    │
│  │  │  └──────────────────────────┘  │   │    │
│  │  └─────────────┬──────────────────┘   │    │
│  │                │                      │    │
│  │                ▼                      │    │
│  │  ┌────────────────────────────────┐   │    │
│  │  │    LLVM Backend (unchanged)    │   │    │
│  │  └────────────────────────────────┘   │    │
│  └──────────────────────────────────────┘    │
│                                              │
│  ┌──────────────────────────────────────┐    │
│  │  rs-macros (single proc-macro crate) │    │
│  │  - #[derive(Addressed)]        500L  │    │
│  │  - #[epoch]                    300L  │    │
│  │  - #[deterministic]            400L  │    │
│  │  - #[register]                 800L  │    │
│  │  - cell! { }                  2000L  │    │
│  └──────────────────────────────────────┘    │
│                                              │
│  ┌──────────────────────────────────────┐    │
│  │  rs (single library crate)            │    │
│  │  - core (+ cyber-hemera)        150L │    │
│  │  - fixed_point                  800L │    │
│  │  - bounded                      600L │    │
│  │  - channel                      500L │    │
│  │  - arena                        400L │    │
│  └──────────────────────────────────────┘    │
│                                              │
└──────────────────────────────────────────────┘
```

## Line Count Breakdown

| Component | Location | Lines | Nature |
|-----------|----------|------:|--------|
| `async(dur)` parser extension | rustc fork | 200 | Compiler patch |
| Bounded async desugaring | rustc fork | 300 | Compiler patch |
| Register MMIO codegen | rustc fork | 800 | Compiler patch |
| Rs edition lint: no heap | rustc fork | 200 | Compiler patch |
| Rs edition lint: no dyn | rustc fork | 50 | Compiler patch |
| Rs edition lint: no panic-unwind | rustc fork | 50 | Compiler patch |
| `#[deterministic]` lint pass | rustc fork | 400 | Compiler patch |
| Bounded async enforcement lint | rustc fork | 200 | Compiler patch |
| Rs diagnostics and error messages | rustc fork | 300 | Compiler patch |
| **Compiler patch subtotal** | | **2,500** | |
| `rs-macros` (all proc-macros) | single proc-macro crate | 4,000 | Standard Rust |
| `rs` (all library code) | single library crate | 2,550 | Standard Rust |
| `cyber-hemera` (Particle/Hemera) | external dep (crates.io) | — | Standard Rust |
| **Crate subtotal** | | **6,550** | |
| **Total** | | **~9,050** | |

The actual rustc patch is ~2,500 lines. Two standard Rust crates (`rs` + `rs-macros`) provide the Phase 1 implementation. Hemera (Poseidon2/Goldilocks hash) is an external dependency (`cyber-hemera` on crates.io).

## Build Pipeline

```bash
# Rs compiler is a patched rustc
$ git clone https://github.com/AnyOrganization/rust.git rsc
$ cd rsc
$ git apply rs-compiler.patch   # ~2,500 lines
$ ./x.py build

# Compiles any .rs file
$ rsc my_program.rs                    # standard Rust mode
$ rsc --edition rs my_program.rs       # Rs mode with all checks

# Or via Cargo
$ cargo +rsc build                     # uses rsc as compiler
```

## Compatibility Testing

CI runs three test suites:

1. **Rust test suite**: the full rustc test suite must pass with rsc (zero regressions)
2. **Top 1000 no_std crates**: compile with rsc to verify superset property
3. **Rs-specific tests**: test all 7 primitives, all error codes, all edge cases
