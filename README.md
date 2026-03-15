---
tags: cyber, rs, rust, language, research
icon: "\u2699\uFE0F"
stake: 2440926101748440
---

# Rs: Safer, Faster, Field-First Rust

> *Rust treats bytes as machine integers. Rs treats bytes as elements of F_p. This single shift makes determinism, addressing, and bounded computation natural rather than enforced.*

Rs is a minimal, strict superset of Rust for building systems where every byte is a field element. Rust manages memory safely. Rs manages *state* safely — where the word is not a machine integer but an element of a finite field, and where correctness means every node produces identical output for identical input.

```
 Universe     Language   Type      Algebra            Purpose
 ─────────────────────────────────────────────────────────────
 Binary       Bt         Bit       F_2 tower          Circuits
 Byte         Rs         Word      Bitwise on F_p     Systems
 Field        Trident    Field     Arithmetic on F_p  Proofs
```

The file extension is `.rs`. The edition identifier is `rs`. The compiler binary is `rsc`.

## Seven Primitives

| # | Primitive | Compiler | Library | Guarantee |
|---|-----------|:-:|:-:|---|
| 1 | [Typed Registers](reference/registers.md) | 800L | — | MMIO without unsafe |
| 2 | [Bounded Async](reference/async.md) | 500L | — | No unbounded waits |
| 3 | [Deterministic Functions](reference/deterministic.md) | 400L | — | Same output everywhere |
| 4 | [Addressed Types](reference/addressed.md) | — | 500L | Identity from content |
| 5 | [Step-Scoped State](reference/step.md) | — | 300L | No state leaks |
| 6 | [Cells](reference/cells.md) | — | 2000L | Hot-swap + lifecycle |
| 7 | [Edition Restrictions](reference/restrictions.md) | 400L | — | No heap, no leaks |

Compiler patch: **~2,500 lines**. Library code: **~5,900 lines**. Rust compatibility: **100%**.

## Documentation

- **[Why Rs Exists](docs/explanation/why.md)** — the algebraic foundation
- **[Design Principles](docs/explanation/design.md)** — superset, editions, zero keywords
- **[Standard Library](reference/stdlib.md)** — fixed_point, bounded, channel, particle, arena
- **[Compiler](reference/compiler.md)** — architecture, line counts, build pipeline
- **[Error Catalog](reference/errors.md)** — all 30 diagnostics (RS001–RS507)
- **[Tutorial: cyb os Cell](docs/tutorials/cyb-cell.md)** — all seven primitives in one file

Any Rust programmer can write Rs. Any LLM trained on Rust can generate Rs. Any no_std crate works with Rs. The ecosystem is not forked — it is extended.

Rust made systems programming safe. Rs makes it algebraic. When the word is a field element, determinism is not a discipline — it is the default.
