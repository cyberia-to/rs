---
tags: cyber, rs, reference
---

# Addressed Types

## Problem

In addressed systems, data is identified by its hash. All addressing reduces to hashing — even physical locations can be geohashed. Producing the hash requires canonical serialization — the same data must always serialize to the same bytes. Rust has no built-in concept of canonical serialization or hash-derived identity.

## Solution

The `Addressed` derive macro generates canonical serialization and a `.particle()` method.

## Syntax

```rust
#[derive(Addressed)]
struct Cyberlink {
    from: Particle,
    to: Particle,
    agent: Address,
    height: u64,
}

let link = Cyberlink { from: a, to: b, agent: alice, height: 100 };
let id: Particle = link.particle();  // Deterministic, canonical hash

// Two identical structs always produce the same Particle
let link2 = Cyberlink { from: a, to: b, agent: alice, height: 100 };
assert_eq!(link.particle(), link2.particle());
```

## Canonical Serialization Rules

The derived serializer follows strict rules:

1. Fields are serialized in declaration order (not alphabetical, not random)
2. Integers are serialized as little-endian fixed-width bytes
3. Variable-length types (arrays) are prefixed with a u32 length
4. No padding bytes between fields
5. Enums serialized as discriminant (u32) + variant data
6. Nested `Addressed` types are serialized as their Particle (64 bytes), not expanded

Hash function: Hemera — a Poseidon2 sponge over the Goldilocks field (p = 2⁶⁴ − 2³² + 1), 64-byte output, producing a Particle. Parameters: state width 16 elements, rate 8, capacity 8, 8 full rounds, 64 partial rounds, S-box x⁷. Capacity 8 (256 bits) provides long-term collision resistance matching the permanence of particle addresses. See the Hemera spec for the full parameter rationale.

## Compile-Time Checks

| Check | Error |
|-------|-------|
| Field type not serializable | `error[RS301]: type MyOpaqueType does not implement CanonicalSerialize` |
| Contains `f32`/`f64` | `error[RS302]: floating point types are not canonically serializable` |
| Contains raw pointers | `error[RS303]: pointers cannot be addressed` |
| Contains `HashMap` | `error[RS304]: HashMap has non-deterministic serialization; use BTreeMap` |

## Implementation

Implemented as a proc-macro (no compiler changes required). Works in both standard Rust and Rs editions. ~500 lines.
