---
tags: cyber, rs, explanation
---

# Design Principles

## Strict Superset

Every valid Rust program is a valid Rs program. This is a hard constraint, not a goal. Compatibility is verified by compiling the top 1000 no_std crates from crates.io with `rsc` on every CI run.

```
Valid Rust ⊂ Valid Rs
```

Rs adds constructs. It never changes the meaning of existing Rust constructs.

## Edition-Gated Restrictions

Rs introduces an `rs` edition. When active, certain Rust features are restricted or enhanced:

```toml
# Cargo.toml
[package]
edition = "rs"
```

In `rs` edition:
- Heap allocation primitives (`Box`, `Vec`, `String`, `HashMap`) produce compile errors unless explicitly opted-in via `#[allow(rs::heap)]`
- `dyn Trait` produces a compile error unless opted-in via `#[allow(rs::dyn_dispatch)]`
- `panic!()` with unwinding produces a compile error; only `abort` mode is permitted
- Floating point types (`f32`, `f64`) are forbidden inside `#[deterministic]` functions
- All `async fn` must have a deadline

In standard Rust editions (`2021`, `2024`), none of these restrictions apply. Rs extensions are still available but optional.

## Zero New Keywords

Rs introduces zero new keywords. All extensions use:
- Attributes (`#[register]`, `#[deterministic]`, `#[step]`, `#[bounded_async]`)
- Derive macros (`#[derive(Addressed)]`)
- Declarative macros (`cell! { }`)

The `async(duration)` shorthand syntax is available inside `cell!` blocks (parsed by the macro). Outside cells, `#[bounded_async(duration)]` is standard Rust attribute syntax.

This ensures no conflict with any existing or future Rust syntax.
