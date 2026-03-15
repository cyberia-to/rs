---
tags: cyber, rs, reference
---

# Addressed Errors (RS301–RS304)

[Back to Error Catalog](../errors.md) | Spec: [addressed.md](../addressed.md)

Enforcement: proc-macro (`#[derive(Addressed)]`) + rsc lint (transitivity verification).

---

### RS301: Type not serializable

```text
error[RS301]: type MyOpaqueType does not implement CanonicalSerialize
  help: derive Addressed on MyOpaqueType, or implement CanonicalSerialize manually
```

Every field in an `Addressed` struct must implement `CanonicalSerialize`. Types without a canonical byte representation cannot be content-addressed.

#### Fix

Derive `Addressed` on the field type, or implement `CanonicalSerialize` manually:

```rust
#[derive(Addressed)]
struct Inner {
    value: u64,
}

#[derive(Addressed)]
struct Outer {
    inner: Inner,  // OK: Inner implements CanonicalSerialize via Addressed
}
```

---

### RS302: Floating point fields

```text
error[RS302]: floating point types are not canonically serializable
  help: use FixedPoint<u128, 18> for deterministic decimal values
```

`f32` and `f64` have multiple bit representations for the same value (NaN variants, ±0). Canonical serialization requires a single byte sequence per value.

#### Fix

Use `FixedPoint<T, DECIMALS>` instead.

---

### RS303: Raw pointer fields

```text
error[RS303]: pointers cannot be addressed
  help: pointers are memory addresses, not content — use the pointed-to value
```

Pointer values depend on memory layout and change between runs. Content addressing requires the actual data, not its location.

#### Fix

Store the data directly or use an index/identifier.

---

### RS304: HashMap fields

```text
error[RS304]: HashMap has non-deterministic serialization; use BTreeMap
  help: HashMap iteration order varies between runs
```

HashMap iteration order is randomized. Serializing a HashMap produces different byte sequences for the same logical data, breaking canonical serialization.

#### Fix

Use `BTreeMap` (deterministic iteration order) or `BoundedMap` (sorted array-backed).
