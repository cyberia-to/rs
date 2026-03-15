---
tags: cyber, rs, reference
---

# Restriction Errors (RS501–RS507)

[Back to Error Catalog](../errors.md) | Spec: [restrictions.md](../restrictions.md)

Enforcement: rsc lint (rs edition only). Opt-out: `#[allow(rs::heap)]` for heap restrictions, `#[allow(rs::dyn_dispatch)]` for dynamic dispatch, `#[allow(rs::nondeterministic)]` for non-deterministic collections. RS506 (unwinding panic) has no opt-out.

---

### RS501: Heap allocation

```text
error[RS501]: heap allocation forbidden in rs edition
  help: use a stack value or Arena<T, N>
```

`Box::new()` allocates on the heap. In rs edition, all allocation must be bounded and explicit.

#### Fix

```rust
// Stack allocation:
let value: u64 = 42;

// Arena allocation (bounded):
let arena: Arena<MyStruct, 1000> = Arena::new();
let item = arena.alloc(MyStruct::new())?;
```

---

### RS502: Growable collections

```text
error[RS502]: growable collections forbidden in rs edition
  help: use BoundedVec<T, N> with compile-time capacity
```

`Vec<T>` can grow without bound via `push()`, triggering unbounded heap allocation. In rs edition, collections must have compile-time capacity.

#### Fix

```rust
let mut v: BoundedVec<u8, 256> = BoundedVec::new();
v.try_push(42)?;  // returns Err if full
```

---

### RS503: Heap-allocated strings

```text
error[RS503]: heap-allocated strings forbidden in rs edition
  help: use &str or ArrayString<N>
```

`String` is a growable heap allocation. Use string slices or fixed-capacity strings.

#### Fix

```rust
let s: &str = "hello";                              // borrowed, no allocation
let s: ArrayString<64> = ArrayString::from("hello"); // fixed capacity
```

---

### RS504: Dynamic dispatch

```text
error[RS504]: dynamic dispatch forbidden in rs edition
  help: use generics or enum dispatch
```

`Box<dyn Trait>` and `&dyn Trait` use vtable-based dispatch with heap allocation (for Box) and indirect calls. In rs edition, use static dispatch via generics or enum dispatch.

Opt-out: `#[allow(rs::dyn_dispatch)]`.

#### Fix

```rust
// Generics (static dispatch):
fn process<T: Handler>(handler: &T) { ... }

// Enum dispatch:
enum Action {
    Read(ReadHandler),
    Write(WriteHandler),
}
```

---

### RS505: Reference counting

```text
error[RS505]: reference counting forbidden in rs edition
  help: use cell-owned state or bounded channels
```

`Arc<T>` and `Rc<T>` use heap allocation and runtime reference counting. In rs edition, ownership is managed by cells and channels.

#### Fix

Use cell state for shared data, or bounded channels for inter-cell communication.

---

### RS506: Unwinding panic

```text
error[RS506]: unwinding panic forbidden in rs edition
  help: use Result for recoverable errors, or abort for unrecoverable
```

Stack unwinding panics add complexity to the runtime (landing pads, drop handlers during unwind). In rs edition, only `panic = "abort"` is permitted.

#### Fix

```rust
// Recoverable: use Result
fn parse(input: &[u8]) -> Result<Data, ParseError> {
    // ...
}

// Unrecoverable: abort is automatic with panic = "abort" in Cargo.toml
[profile.release]
panic = "abort"
```

---

### RS507: Non-deterministic collections

```text
error[RS507]: non-deterministic collections forbidden in rs edition
  help: use BTreeSet for deterministic iteration order
```

`HashMap` and `HashSet` use a randomized hasher — iteration order varies between runs. Forbidden in rs edition for determinism.

Opt-out: `#[allow(rs::nondeterministic)]`.

#### Fix

```rust
use std::collections::BTreeSet;
let s: BTreeSet<u32> = BTreeSet::new();  // deterministic iteration

// Or use BoundedMap for bounded + deterministic:
let m: BoundedMap<Key, Value, 1000> = BoundedMap::new();
```
