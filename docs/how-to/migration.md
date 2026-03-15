---
tags: cyber, rs, how-to
---

# Migration Path

## Phase 1: Library-Only (works today)

Before the compiler patch exists, all Rs concepts except two can be used as standard Rust crates:

| Primitive | Library Implementation | Limitation |
|-----------|----------------------|------------|
| Typed registers | `rs-registers` proc-macro | No compiler verification of MMIO safety |
| Bounded async | `rs-async` with timeout wrapper | Not enforced, opt-in |
| Deterministic functions | `rs-deterministic` proc-macro | Partial: catches float but not all cases |
| Addressed types | `rs-cid` derive macro (Hemera) | Full functionality |
| Epoch-scoped state | `rs-epoch` attribute macro | No cross-cell enforcement |
| Cell declarations | `rs-cell` proc-macro | Full functionality |
| Owned regions | `rs-lint` clippy plugin | Advisory warnings, not errors |

This means cyb os development can start immediately using standard Rust with Rs libraries.

## Phase 2: Compiler Patch

Apply the ~2,450 line patch to rustc. All library-based Rs code continues to work. Compiler now also enforces:
- MMIO safety at compile time
- Bounded async as requirement in Rs edition
- Deterministic function purity
- Heap/dyn restrictions in Rs edition

## Phase 3: Upstream

Propose individual Rs features as Rust RFCs where appropriate:
- `#[deterministic]` has general value beyond cyb os
- Bounded async could benefit any reliability-critical Rust code
- Typed registers would benefit the entire embedded Rust ecosystem

Features that are too domain-specific remain in the Rs fork.
