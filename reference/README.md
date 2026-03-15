---
tags: cyber, rs, reference
---

# Rs Reference

## Primitives

| # | Primitive | Reference | Type |
|---|-----------|-----------|------|
| 1 | [Typed Registers](registers.md) | `#[register]` | Compiler (800L) |
| 2 | [Bounded Async](async.md) | `async(duration)` | Compiler (500L) |
| 3 | [Deterministic Functions](deterministic.md) | `#[deterministic]` | Compiler (400L) |
| 4 | [Addressed Types](addressed.md) | `#[derive(Addressed)]` | Proc-macro (500L) |
| 5 | [Epoch-Scoped State](epoch.md) | `#[epoch]` | Proc-macro (300L) |
| 6 | [Cell Declarations](cells.md) | `cell! { }` | Proc-macro (2000L) |
| 7 | [Owned Regions](regions.md) | `edition = "rs"` | Compiler (250L) |

## Infrastructure

- [Standard Library](stdlib.md) — fixed_point, bounded, channel, particle, arena
- [Compiler](compiler.md) — architecture, line counts, build pipeline
