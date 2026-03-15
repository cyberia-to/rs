---
tags: cyber, rs, reference
---

# Step Errors (RS401)

[Back to Error Catalog](../errors.md) | Spec: [step.md](../step.md)

Enforcement: rsc lint (rs edition only).

---

### RS401: Step state outside cell context

```text
error[RS401]: #[step] state accessed outside of cell context
  help: #[step] state must be accessed within a cell! block
  help: step reset is managed by the cell runtime
```

`#[step]` state is automatically reset at step boundaries by the cell runtime. Accessing it outside a cell context means no runtime manages its lifecycle — the reset would never happen, defeating the purpose of step scoping.

#### Fix

Access step state from within a cell:

```rust
cell! {
    name: MyCell,
    step_state {
        counter: u64,
    }

    pub fn tick(&mut self) {
        self.step_state.counter += 1;  // OK: inside cell context
    }
}
```
