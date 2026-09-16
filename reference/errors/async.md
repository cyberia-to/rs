---
tags: cyber, rs, reference
---

# Async Errors (RS101)

[Back to Error Catalog](../errors.md) | Spec: [async.md](../async.md)

Enforcement: rsc lint (rs edition only).

---

### RS101: Unbounded async function

```text
error[RS101]: async functions must have a deadline in rs edition
  help: add a deadline: async(Duration::from_millis(100)) fn unbounded()
  help: or opt out: #[allow(rs::unbounded_async)]
```

In `edition = "rs"`, every async function must have an explicit deadline. An async function without a deadline can block indefinitely — a liveness failure in OS kernels and consensus nodes.

Inside `module!` macro: use `async(Duration) fn` syntax.
Outside modules: use `#[bounded_async(Duration)]` attribute macro.

#### Fix

```rust
// Inside module!:
pub async(Duration::from_millis(100)) fn fetch(&self) -> Result<Data> {
    // ...
}

// Outside module!:
#[bounded_async(Duration::from_millis(100))]
async fn fetch() -> Result<Data, AppError> {
    // ...
}

// Explicit opt-out (must justify):
#[allow(rs::unbounded_async)]
async fn special_case() -> Result<()> {
    // ...
}
```
