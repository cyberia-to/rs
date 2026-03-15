---
tags: cyber, rs, reference
---

# Register Errors (RS001–RS008)

[Back to Error Catalog](../errors.md) | Spec: [registers.md](../registers.md)

Enforcement: proc-macro (`#[register]` attribute).

---

### RS001: Read from write-only register

```text
error[RS001]: register aic::Clear is write-only
```

Write-only registers (access = "wo") have no `read()` method. Attempting to read a write-only register is a hardware error — the value returned would be undefined.

#### Fix

Use a read-write or read-only register, or remove the read call.

---

### RS002: Write to read-only register

```text
error[RS002]: register aic::Status is read-only
```

Read-only registers (access = "ro") have no `write()` or `modify()` method. Attempting to write to a read-only register is a hardware error.

#### Fix

Use a read-write or write-only register, or remove the write call.

---

### RS003: Field exceeds register width

```text
error[RS003]: field target_cpu (bits 1..5) exceeds u32 width
```

A field's bit range extends beyond the register's declared width. A u32 register has bits 0..31.

#### Fix

Adjust the field's bit range to fit within the register width, or increase the register width.

---

### RS004: Field value exceeds bit range

```text
error[RS004]: value 20 does not fit in 4-bit field target_cpu
```

A constant value assigned to a field is too large for the field's bit width. A 4-bit field can hold values 0–15.

#### Fix

Use a value that fits within the field width, or widen the field.

---

### RS005: Overlapping field bits

```text
error[RS005]: fields enabled and target_cpu overlap at bit 1
```

Two fields in the same register declare overlapping bit ranges. Each bit in a register must belong to at most one field.

#### Fix

Adjust bit ranges so fields don't overlap.

---

### RS006: Enum variant exceeds field width

```text
error[RS006]: Priority has 5 variants but field priority is 2 bits (max 4)
```

An enum used as a field type has more variants than the field's bit width can represent. A 2-bit field can hold at most 4 values.

#### Fix

Remove variants or widen the field.

```rust
// 5 variants cannot fit in 2 bits (max 4):
#[repr(u8)]
enum Priority {
    Idle = 0,
    Low = 1,
    Normal = 2,
    High = 3,
    Critical = 4,  // error: no room in 2 bits
}

// Fix: widen to 3 bits (max 8), or remove a variant
#[field(bits = 0..3)]
pub priority: Priority,
```

---

### RS007: Address outside declared bank

```text
error[RS007]: offset 0x2000 exceeds bank_size 0x1000
```

A register's offset is outside the declared bank_size of the register module.

#### Fix

Use an offset within the bank size, or increase bank_size.

---

### RS008: Enum does not cover all bit patterns

```text
error[RS008]: IrqMode has 3 variants but field mode is 2 bits (4 patterns) — add a variant for pattern 3
```

An enum used as a field type does not cover all possible bit patterns for the field width. Hardware can return any bit pattern — unmapped patterns would cause undefined behavior in the match.

#### Fix

Add variants to cover all bit patterns. For a 2-bit field, 4 variants are needed. Use a `Reserved` variant for unused patterns:

```rust
#[repr(u8)]
enum IrqMode {
    Level = 0,
    Edge = 1,
    Hybrid = 2,
    Reserved = 3,
}
```
