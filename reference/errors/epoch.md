---
tags: cyber, rs, reference
---

# Epoch Errors (RS401)

[Back to Error Catalog](../errors.md) | Spec: [epoch.md](../epoch.md)

Enforcement: rsc lint (rs edition only).

---

### RS401: Epoch state outside cell context

```text
error[RS401]: #[epoch] state accessed outside of cell context
  help: #[epoch] state must be accessed within a cell! block
  help: epoch reset is managed by the cell runtime
```

`#[epoch]` state is automatically reset at epoch boundaries by the cell runtime. Accessing it outside a cell context means no runtime manages its lifecycle — the reset would never happen, defeating the purpose of epoch scoping.

#### Fix

Access epoch state from within a cell:

```rust
cell! {
    name: MyCell,
    epoch_state {
        counter: u64,
    }

    pub fn tick(&mut self) {
        self.epoch_state.counter += 1;  // OK: inside cell context
    }
}
```
