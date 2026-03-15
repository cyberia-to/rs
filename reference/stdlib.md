---
tags: cyber, rs, reference
---

# Rs Standard Library Extensions

## `rs::fixed_point`

```rust
use rs::fixed_point::FixedPoint;

type Amount = FixedPoint<u128, 18>;  // 18 decimal places

let a = Amount::from_integer(100);
let b = Amount::from_decimal(3, 14);  // 3.14
let c = a.checked_mul(b).unwrap();    // 314.00...
let d = a.checked_div(b).unwrap();    // 31.847...

// All operations: checked_add, checked_sub, checked_mul, checked_div
// All deterministic, all return Option (no panics)
```

## `rs::bounded`

```rust
use rs::bounded::{BoundedVec, BoundedMap, ArrayString};

let mut v: BoundedVec<u8, 256> = BoundedVec::new();
v.try_push(42)?;  // Returns Err if full

let mut m: BoundedMap<Key, Value, 1000> = BoundedMap::new();
m.try_insert(k, v)?;  // Returns Err if full

let s: ArrayString<64> = ArrayString::try_from("hello")?;
```

## `rs::channel`

```rust
use rs::channel::{bounded_channel, Sender, Receiver};

// Wait-free bounded MPMC channel
let (tx, rx) = bounded_channel::<Transaction>(1000);

// Non-blocking send
match tx.try_send(transaction) {
    Ok(()) => { /* sent */ }
    Err(Full(tx)) => { /* channel full, backpressure */ }
}

// Bounded receive (with deadline from async context)
let msg = rx.recv().await;  // Inherits caller's deadline
```

## `rs::cid`

```rust
use rs::cid::Cid;

let data = b"hello world";
let cid = Cid::from_bytes(data);  // Hemera hash

// Cid is Copy, 64 bytes (8 Goldilocks field elements), comparable, hashable
let map: BoundedMap<Cid, Data, 10_000> = BoundedMap::new();
```

## `rs::arena`

```rust
use rs::arena::Arena;

let arena: Arena<MyStruct, 5000> = Arena::new();
let item: &mut MyStruct = arena.alloc(MyStruct::new())?;

// Arena tracks count, provides iteration
assert!(arena.count() <= 5000);
for item in arena.iter() { /* ... */ }

// All freed on drop — no individual deallocation
```
