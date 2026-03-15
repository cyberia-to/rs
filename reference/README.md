---
tags: cyber, rs, reference
---

# Rs Reference

## Primitives

| # | Primitive | Reference | Type |
|---|-----------|-----------|------|
| 1 | [Typed Registers](registers.md) | `#[register]` | Compiler (800L) |
| 2 | [Bounded Async](async.md) | `async(duration)` / `#[bounded_async]` | Proc-macro (200L) + rsc lint (200L) |
| 3 | [Deterministic Functions](deterministic.md) | `#[deterministic]` | Compiler (400L) |
| 4 | [Addressed Types](addressed.md) | `#[derive(Addressed)]` | Proc-macro (500L) |
| 5 | [Step-Scoped State](step.md) | `#[step]` | Proc-macro (300L) |
| 6 | [Cell Declarations](cells.md) | `cell! { }` | Proc-macro (2000L) |
| 7 | [Edition Restrictions](restrictions.md) | `edition = "rs"` | Compiler (400L) |

## Infrastructure

- [Standard Library](stdlib.md) — fixed_point, bounded, channel, particle, arena
- [Compiler](compiler.md) — architecture, line counts, build pipeline
- [Error Catalog](errors.md) — all 30 diagnostics (RS001–RS507)
