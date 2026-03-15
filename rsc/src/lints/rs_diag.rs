//! Rs diagnostic definitions: error codes RS001-RS507.
//!
//! Standalone module — no rustc internal dependencies. Used for
//! --explain and --rs-list-errors.

pub struct RsDiagnostic {
    pub code: &'static str,
    pub short: &'static str,
    pub long: &'static str,
    pub suggestion: &'static str,
}

pub static RS_DIAGNOSTICS: &[RsDiagnostic] = &[
    RsDiagnostic {
        code: "RS001", short: "read from write-only register",
        long: "Write-only registers (access = \"wo\") do not retain their value after a write, so reading them would return undefined data. The #[register] macro enforces this at compile time by not generating a read() method.",
        suggestion: "use a read-write or read-only register",
    },
    RsDiagnostic {
        code: "RS002", short: "write to read-only register",
        long: "Read-only registers (access = \"ro\") are hardwired by the peripheral and cannot be changed by software. The #[register] macro enforces this by not generating write() or modify() methods.",
        suggestion: "use a read-write or write-only register",
    },
    RsDiagnostic {
        code: "RS003", short: "field exceeds register width",
        long: "A register field's bit range [high:low] extends beyond the register's declared width. For example, bits 33:32 in a 32-bit register. Adjust the bit range or use a wider register type.",
        suggestion: "adjust the field's bit range or increase the register width",
    },
    RsDiagnostic {
        code: "RS004", short: "field value exceeds bit range",
        long: "A constant value assigned to a register field exceeds the maximum value representable by the field's bit width. A 3-bit field can hold values 0-7.",
        suggestion: "use a value that fits within the field width",
    },
    RsDiagnostic {
        code: "RS005", short: "overlapping field bits",
        long: "Two register fields claim overlapping bit positions within the same register. Each bit in a register must belong to exactly one field (or be reserved).",
        suggestion: "adjust bit ranges so fields don't overlap",
    },
    RsDiagnostic {
        code: "RS006", short: "enum variant exceeds field width",
        long: "An enum used as a register field value has more variants than the field's bit width can represent. A 2-bit field can represent at most 4 variants.",
        suggestion: "remove variants or widen the field",
    },
    RsDiagnostic {
        code: "RS007", short: "address outside declared bank",
        long: "A register's byte offset exceeds the declared bank_size for the peripheral. This would access memory outside the peripheral's mapped region.",
        suggestion: "use an offset within the bank size",
    },
    RsDiagnostic {
        code: "RS008", short: "enum does not cover all bit patterns",
        long: "An enum used as a register field does not cover every possible bit pattern for the field width. A 2-bit field requires exactly 4 variants (or a catch-all). Add a Reserved variant to cover unused patterns.",
        suggestion: "add a Reserved variant",
    },
    RsDiagnostic {
        code: "RS101", short: "async function must have a deadline",
        long: "Every async function in Rs must declare an explicit time budget. Unbounded async operations can stall a cell indefinitely, violating the system's liveness guarantees. Use #[bounded_async(Duration::from_millis(N))] or declare the function inside a cell! block with async(Duration) syntax.",
        suggestion: "#[bounded_async(Duration::from_millis(100))]",
    },
    RsDiagnostic {
        code: "RS201", short: "floating point in #[deterministic] function",
        long: "Floating-point arithmetic (f32/f64) produces different results on different CPU architectures due to extended precision, fused multiply-add, and rounding mode differences. Use FixedPoint<u64, N> or FixedPoint<u128, N> for deterministic decimal arithmetic.",
        suggestion: "use FixedPoint<u128, 18>",
    },
    RsDiagnostic {
        code: "RS202", short: "float cast in #[deterministic] function",
        long: "Casts between float and integer types have platform-dependent rounding behavior. The same cast can produce different results on x86 (80-bit extended) vs ARM (64-bit strict). Use explicit integer arithmetic or FixedPoint conversion methods.",
        suggestion: "use integer arithmetic or FixedPoint",
    },
    RsDiagnostic {
        code: "RS203", short: "raw pointer arithmetic in #[deterministic] function",
        long: "Raw pointer values (addresses) are non-deterministic \u{2014} they change between runs due to ASLR, allocator state, and stack layout. Pointer arithmetic in a deterministic context would produce different results on each execution. Use index-based access into arrays or bounded collections.",
        suggestion: "use index-based access",
    },
    RsDiagnostic {
        code: "RS204", short: "system clock in #[deterministic] function",
        long: "System clocks (Instant::now(), SystemTime::now()) return wall-clock or monotonic time that varies between machines and runs. Deterministic functions must use logical step counters provided by the cell runtime instead.",
        suggestion: "use step counters",
    },
    RsDiagnostic {
        code: "RS205", short: "randomness in #[deterministic] function",
        long: "Random number generators produce different sequences on each invocation (unless seeded). Any function reachable from a #[deterministic] context must avoid randomness. Use deterministic seed-based computation if pseudorandom values are needed.",
        suggestion: "use deterministic seed-based computation",
    },
    RsDiagnostic {
        code: "RS206", short: "unchecked arithmetic in #[deterministic] function",
        long: "The default arithmetic operators (+, -, *, /) have different overflow behavior in debug (panic) vs release (wrapping) builds. This means the same code produces different results depending on the build profile. Use checked_add/checked_sub/checked_mul/checked_div which return Option and behave identically in all build modes.",
        suggestion: "use checked_add, checked_sub, checked_mul",
    },
    RsDiagnostic {
        code: "RS207", short: "HashMap in #[deterministic] function",
        long: "HashMap and HashSet use a randomized hash function (SipHash with per-process random keys). Iteration order changes between runs and between processes. Use BTreeMap/BTreeSet for deterministic sorted iteration, or BoundedMap for fixed-capacity sorted maps.",
        suggestion: "use BTreeMap",
    },
    RsDiagnostic {
        code: "RS208", short: "inline assembly in #[deterministic] function",
        long: "Inline assembly (asm!, global_asm!) is inherently platform-specific and cannot be verified for determinism by the compiler. Deterministic functions must use portable Rust code.",
        suggestion: "use portable Rust code",
    },
    RsDiagnostic {
        code: "RS209", short: "non-deterministic callee in #[deterministic] function",
        long: "Determinism is a transitive property \u{2014} a #[deterministic] function that calls a non-deterministic function is itself non-deterministic. Every function in the call graph must be either #[deterministic] or const fn.",
        suggestion: "mark callee #[deterministic] or const fn",
    },
    RsDiagnostic {
        code: "RS210", short: "usize/isize in #[deterministic] function",
        long: "usize and isize have platform-dependent width (32-bit on 32-bit targets, 64-bit on 64-bit targets). Arithmetic involving these types produces different results on different platforms. Use u32 or u64 for fixed-width integers.",
        suggestion: "use u32 or u64",
    },
    RsDiagnostic {
        code: "RS301", short: "type does not implement CanonicalSerialize",
        long: "Every field in an Addressed struct must implement CanonicalSerialize to produce a deterministic byte representation. Without canonical serialization, the Particle (content hash) would be meaningless.",
        suggestion: "derive Addressed or implement CanonicalSerialize",
    },
    RsDiagnostic {
        code: "RS302", short: "float in Addressed type",
        long: "Floating-point values have multiple bit representations for the same mathematical value (e.g., +0.0 and -0.0, denormals). This makes canonical serialization impossible \u{2014} the same logical value could produce different hashes. Use FixedPoint for deterministic numeric representation.",
        suggestion: "use FixedPoint",
    },
    RsDiagnostic {
        code: "RS303", short: "pointer in Addressed type",
        long: "Pointer values are memory addresses that change between program runs. Including them in a canonical serialization would make the Particle non-deterministic. Store the pointed-to data directly.",
        suggestion: "store data directly",
    },
    RsDiagnostic {
        code: "RS304", short: "HashMap in Addressed type",
        long: "HashMap serialization iterates over entries in hash-table order, which is randomized. The same logical map would serialize to different byte sequences, producing different Particles. Use BTreeMap (sorted) or BoundedMap (sorted, fixed-capacity).",
        suggestion: "use BTreeMap or BoundedMap",
    },
    RsDiagnostic {
        code: "RS305", short: "usize/isize in Addressed type",
        long: "usize and isize serialize to different byte widths on different platforms (4 bytes on 32-bit, 8 bytes on 64-bit). This breaks canonical serialization \u{2014} the same value produces different byte sequences. Use u32 or u64 for platform-independent serialization.",
        suggestion: "use u32 or u64",
    },
    RsDiagnostic {
        code: "RS306", short: "enum discriminant exceeds u32",
        long: "Addressed enums serialize their discriminant as a u32. Enum representations wider than u32 (#[repr(u64)]) would truncate the discriminant during serialization. Use #[repr(u8)], #[repr(u16)], or #[repr(u32)].",
        suggestion: "use #[repr(u8/u16/u32)]",
    },
    RsDiagnostic {
        code: "RS401", short: "#[step] state outside cell context",
        long: "Step-scoped state (#[step] structs) is managed by the cell runtime and automatically reset at step boundaries. Accessing it outside a cell context bypasses the runtime's lifecycle management, leading to stale or uninitialized state.",
        suggestion: "access from within a cell! block",
    },
    RsDiagnostic {
        code: "RS501", short: "heap allocation forbidden",
        long: "Box<T> allocates memory on the heap at runtime. Rs edition code must use stack allocation or Arena<T, N> for fixed-capacity bump allocation. This ensures all memory usage is bounded and predictable.",
        suggestion: "use Arena<T, N>",
    },
    RsDiagnostic {
        code: "RS502", short: "growable collections forbidden",
        long: "Vec<T> is a growable heap collection that can reallocate at any push(). Rs edition requires compile-time-bounded collections. Use BoundedVec<T, N> which has fixed capacity N and never reallocates.",
        suggestion: "use BoundedVec<T, N>",
    },
    RsDiagnostic {
        code: "RS503", short: "heap strings forbidden",
        long: "String is a growable heap-allocated UTF-8 buffer (essentially Vec<u8>). Rs edition requires bounded string storage. Use &str for borrowed strings or ArrayString<N> for owned fixed-capacity strings.",
        suggestion: "use &str or ArrayString<N>",
    },
    RsDiagnostic {
        code: "RS504", short: "dynamic dispatch forbidden",
        long: "Dynamic dispatch (dyn Trait) uses vtable-based indirect calls, which prevent static analysis, inlining, and monomorphization. Use generic type parameters (impl Trait) or enum dispatch for static dispatch.",
        suggestion: "use generics or enum dispatch",
    },
    RsDiagnostic {
        code: "RS505", short: "reference counting forbidden",
        long: "Arc and Rc use heap allocation for the reference-counted pointer and runtime atomic/non-atomic counting for the reference count. Rs edition requires explicit ownership through cell state or bounded channels.",
        suggestion: "use cell-owned state",
    },
    RsDiagnostic {
        code: "RS506", short: "unwinding panic forbidden",
        long: "Stack unwinding (panic = \"unwind\") adds landing pads to every function call, increasing binary size and preventing certain optimizations. Rs edition requires panic = \"abort\" which terminates immediately on panic. Set panic = \"abort\" in the [profile.*] sections of Cargo.toml.",
        suggestion: "set panic = \"abort\"",
    },
    RsDiagnostic {
        code: "RS507", short: "non-deterministic collections forbidden",
        long: "HashMap and HashSet use randomized hashing (SipHash with per-process random keys), making iteration order non-deterministic. Use BTreeMap/BTreeSet for sorted deterministic iteration, or BoundedMap for fixed-capacity sorted maps.",
        suggestion: "use BTreeSet",
    },
];

pub fn lookup(code: &str) -> Option<&'static RsDiagnostic> {
    RS_DIAGNOSTICS.iter().find(|d| d.code == code)
}

pub fn explain(code: &str) -> Option<String> {
    lookup(code).map(|d| {
        format!(
            "error[{}]: {}\n\n{}\n\nSuggestion: {}\n",
            d.code, d.short, d.long, d.suggestion
        )
    })
}

pub fn list_all() -> String {
    let mut out = String::with_capacity(4096);
    for d in RS_DIAGNOSTICS {
        out.push_str(&format!("{}: {}\n", d.code, d.short));
    }
    out
}
