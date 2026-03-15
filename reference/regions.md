---
tags: cyber, rs, reference
---

# Owned Regions

## Problem

Rust's borrow checker is designed for complex ownership graphs: multiple references with different lifetimes, interior mutability, self-referential structs. This complexity exists to support general-purpose programming. An OS kernel for a blockchain node doesn't need most of it.

## Solution

In `edition = "rs"`, the compiler enforces a simpler ownership model through lints. This is not a language change — it's a restriction.

## Restrictions

```rust
// edition = "rs" (set in Cargo.toml or via rsc --edition rs)

// FORBIDDEN (unless opted-in):

let x = Box::new(42);
//~^ error[RS501]: heap allocation forbidden in rs edition
//~| help: use a stack value or Arena<T, N>

let v = Vec::new();
//~^ error[RS502]: growable collections forbidden in rs edition
//~| help: use BoundedVec<T, N> with compile-time capacity

let s = String::from("hello");
//~^ error[RS503]: heap-allocated strings forbidden in rs edition
//~| help: use &str or ArrayString<N>

let d: Box<dyn Trait> = ...;
//~^ error[RS504]: dynamic dispatch forbidden in rs edition
//~| help: use generics or enum dispatch

let r = Arc::new(data);
let r2 = Rc::new(data);
//~^ error[RS505]: reference counting forbidden in rs edition
//~| help: use cell-owned state or bounded channels

let s: HashSet<u32> = HashSet::new();
//~^ error[RS507]: non-deterministic collections forbidden in rs edition
//~| help: use BTreeSet for deterministic iteration order

// ALLOWED:

let a: [u8; 1024] = [0; 1024];                          // stack allocation
let arena: Arena<Transaction, 10_000> = Arena::new();    // typed arena, compile-time sized
let bv: BoundedVec<u8, 256> = BoundedVec::new();        // bounded, no heap
let s: ArrayString<64> = ArrayString::from("hello");     // fixed-capacity string
```

## Arena Allocator

Rs provides a built-in arena type for cases where dynamic-count-but-bounded allocation is needed:

```rust
// Arena with compile-time maximum capacity
let arena: Arena<Transaction, 10_000> = Arena::new();

// Allocate returns Option — None if arena is full
let tx: &mut Transaction = arena.alloc(Transaction::default())?;

// All allocations freed when arena goes out of scope
// No individual deallocation — this is by design
// No fragmentation, no use-after-free, no leaks
```

## Panic Restriction

In `edition = "rs"`, unwinding panics are forbidden:

```rust
// edition = "rs"

panic!("this will not compile");
//~^ error[RS506]: unwinding panic forbidden in rs edition
//~| help: use Result for recoverable errors, or abort for unrecoverable
```

Only `panic = "abort"` is permitted. This ensures stack unwinding never occurs, simplifying the runtime and making resource cleanup explicit.

## Explicit Opt-In

For interfacing with existing Rust crates that use heap allocation:

```rust
#[allow(rs::heap)]
mod legacy_compat {
    use some_crate::SomeType;  // This crate uses Vec internally
    // OK: heap restrictions lifted in this module
}
```

Implementation: ~400 lines (lint passes: no-heap ~200L, no-dyn ~50L, no-panic-unwind ~50L, diagnostics ~100L).

## Error Reference

See [errors/regions.md](errors/regions.md) for detailed descriptions of RS501–RS507.
