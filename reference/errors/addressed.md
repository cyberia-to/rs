---
tags: cyber, rs, reference
---

# Addressed Errors (RS301–RS306)

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

---

### RS305: Platform-dependent integer fields

```text
error[RS305]: usize/isize width is platform-dependent; use u32 or u64
  help: canonical serialization requires fixed-width integers
```

`usize` and `isize` serialize to different byte widths on different platforms (4 bytes on 32-bit, 8 bytes on 64-bit). Canonical serialization requires every value to produce identical bytes regardless of platform.

#### Fix

Replace `usize`/`isize` fields with `u32` or `u64`:

```rust
#[derive(Addressed)]
struct Entry {
    index: u32,   // not usize
    offset: u64,  // not usize
}
```

---

### RS306: Enum repr wider than u32

```text
error[RS306]: Addressed enum discriminant must fit in u32; #[repr(u64)] is not supported
  help: canonical serialization encodes enum discriminants as u32
```

Addressed enums serialize discriminants as `u32` (4 bytes, little-endian). An enum with `#[repr(u64)]` could have discriminant values exceeding `u32::MAX`, which would be truncated during serialization.

#### Fix

Use `#[repr(u8)]`, `#[repr(u16)]`, or `#[repr(u32)]`:

```rust
#[derive(Addressed)]
#[repr(u16)]
enum Status {
    Active = 0,
    Inactive = 1,
    Suspended = 2,
}
