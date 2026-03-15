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
│  │  Rs Proc-Macros (standard crates)    │    │
│  │  - #[derive(Addressed)]        500L  │    │
│  │  - cell! { } macro          2000L    │    │
│  │  - #[epoch] handling         300L    │    │
│  └──────────────────────────────────────┘    │
│                                              │
│  ┌──────────────────────────────────────┐    │
│  │  Rs Standard Library                  │    │
│  │  - rs::fixed_point           800L    │    │
│  │  - rs::bounded               600L    │    │
│  │  - rs::channel               500L    │    │
│  │  - rs::cid (Hemera)          800L    │    │
│  │  - rs::arena                 400L    │    │
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
| `#[deterministic]` lint pass | rustc fork | 400 | Compiler patch |
| Bounded async enforcement lint | rustc fork | 200 | Compiler patch |
| Rs diagnostics and error messages | rustc fork | 300 | Compiler patch |
| **Compiler patch subtotal** | | **2,450** | |
| `#[derive(Addressed)]` | proc-macro crate | 500 | Standard Rust |
| `cell!` macro | proc-macro crate | 2,000 | Standard Rust |
| `#[epoch]` attribute | proc-macro crate | 300 | Standard Rust |
| **Proc-macro subtotal** | | **2,800** | |
| `rs::fixed_point` | library crate | 800 | Standard Rust |
| `rs::bounded` | library crate | 600 | Standard Rust |
| `rs::channel` | library crate | 500 | Standard Rust |
| `rs::cid` (Hemera) | library crate | 800 | Standard Rust |
| `rs::arena` | library crate | 400 | Standard Rust |
| **Library subtotal** | | **3,100** | |
| **Total** | | **~8,350** | |

The actual rustc patch is ~2,450 lines. Everything else is standard Rust crates that work with both `rsc` and `rustc`.

## Build Pipeline

```bash
# Rs compiler is a patched rustc
$ git clone https://github.com/AnyOrganization/rust.git rsc
$ cd rsc
$ git apply rs-compiler.patch   # ~2,450 lines
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
