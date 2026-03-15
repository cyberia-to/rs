---
tags: cyber, rs, reference
---

# Error Catalog

30 diagnostics across 6 categories. All error codes are stable — proc-macros and compiler lints emit identical codes.

| Category | Codes | Count | Spec | Enforcement |
|----------|-------|------:|------|-------------|
| [Registers](errors/registers.md) | RS001–RS008 | 8 | [registers.md](registers.md) | proc-macro |
| [Async](errors/async.md) | RS101 | 1 | [async.md](async.md) | rsc lint |
| [Deterministic](errors/deterministic.md) | RS201–RS209 | 9 | [deterministic.md](deterministic.md) | proc-macro (partial) + rsc lint (full) |
| [Addressed](errors/addressed.md) | RS301–RS304 | 4 | [addressed.md](addressed.md) | proc-macro + rsc lint |
| [Step](errors/step.md) | RS401 | 1 | [step.md](step.md) | rsc lint |
| [Restrictions](errors/restrictions.md) | RS501–RS507 | 7 | [restrictions.md](restrictions.md) | rsc lint |

## Code Ranges

```
RS0xx — Typed registers (MMIO validation)
RS1xx — Bounded async (deadline enforcement)
RS2xx — Deterministic functions (purity checks)
RS3xx — Addressed types (canonical serialization)
RS4xx — Step-scoped state (context enforcement)
RS5xx — Edition restrictions (allocation, dispatch, control flow)
```

## Enforcement Levels

Each error code is enforced at one or both levels:

- **proc-macro**: works with standard rustc via rs-lang-macros
- **rsc lint**: requires rsc compiler driver, operates on HIR/MIR

| Level | What it checks | Strength |
|-------|---------------|----------|
| proc-macro | Token stream / syn AST | Catches literal usage, misses indirection |
| rsc lint | HIR types, MIR call graph | Full transitive analysis |

Codes enforced by proc-macro only: RS001–RS008, RS301–RS304 (partial).
Codes enforced by rsc only: RS101, RS206, RS209, RS401, RS501–RS507.
Codes enforced by both: RS201–RS205, RS207, RS208, RS301–RS304.
