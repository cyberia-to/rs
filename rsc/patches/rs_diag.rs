//! Rs diagnostic definitions: error codes RS001–RS507.
//!
//! Each error code has a short description (used in compiler output) and a
//! long explanation (used by `rsc --explain RSxxx`). Error codes are stable
//! and match the spec in reference/errors.md.

use rustc_errors::DiagnosticMessage;

/// All Rs error codes with short and long descriptions.
///
/// The short description appears in compiler error output.
/// The long explanation appears when the user runs `rsc --explain RSxxx`.
pub struct RsDiagnostic {
    pub code: &'static str,
    pub short: &'static str,
    pub long: &'static str,
    pub suggestion: &'static str,
}

/// Complete catalog of Rs diagnostics, indexed by code.
pub static RS_DIAGNOSTICS: &[RsDiagnostic] = &[
    // -----------------------------------------------------------------------
    // RS0xx: Typed registers (proc-macro enforced, listed for --explain)
    // -----------------------------------------------------------------------
    RsDiagnostic {
        code: "RS001",
        short: "read from write-only register",
        long: "Write-only registers (access = \"wo\") have no read() method. \
               Attempting to read a write-only register is a hardware error \
               — the value returned would be undefined.",
        suggestion: "use a read-write or read-only register, or remove the read call",
    },
    RsDiagnostic {
        code: "RS002",
        short: "write to read-only register",
        long: "Read-only registers (access = \"ro\") have no write() or modify() \
               method. Attempting to write to a read-only register is a hardware error.",
        suggestion: "use a read-write or write-only register, or remove the write call",
    },
    RsDiagnostic {
        code: "RS003",
        short: "field exceeds register width",
        long: "A field's bit range extends beyond the register's declared width. \
               A u32 register has bits 0..31.",
        suggestion: "adjust the field's bit range or increase the register width",
    },
    RsDiagnostic {
        code: "RS004",
        short: "field value exceeds bit range",
        long: "A constant value assigned to a field is too large for the field's \
               bit width. A 4-bit field can hold values 0-15.",
        suggestion: "use a value that fits within the field width, or widen the field",
    },
    RsDiagnostic {
        code: "RS005",
        short: "overlapping field bits",
        long: "Two fields in the same register declare overlapping bit ranges. \
               Each bit in a register must belong to at most one field.",
        suggestion: "adjust bit ranges so fields don't overlap",
    },
    RsDiagnostic {
        code: "RS006",
        short: "enum variant exceeds field width",
        long: "An enum used as a field type has more variants than the field's \
               bit width can represent. A 2-bit field can hold at most 4 values.",
        suggestion: "remove variants or widen the field",
    },
    RsDiagnostic {
        code: "RS007",
        short: "address outside declared bank",
        long: "A register's offset is outside the declared bank_size of the \
               register module.",
        suggestion: "use an offset within the bank size, or increase bank_size",
    },
    RsDiagnostic {
        code: "RS008",
        short: "enum does not cover all bit patterns",
        long: "An enum used as a field type does not cover all possible bit \
               patterns for the field width. Hardware can return any bit \
               pattern — unmapped patterns would cause undefined behavior.",
        suggestion: "add a Reserved variant for unused patterns",
    },

    // -----------------------------------------------------------------------
    // RS1xx: Bounded async
    // -----------------------------------------------------------------------
    RsDiagnostic {
        code: "RS101",
        short: "async functions must have a deadline in rs edition",
        long: "In edition = \"rs\", every async function must have an explicit \
               deadline. An async function without a deadline can block \
               indefinitely — a liveness failure in OS kernels and consensus \
               nodes.\n\n\
               Inside cell! macro: use `async(Duration) fn` syntax.\n\
               Outside cells: use `#[bounded_async(Duration)]` attribute macro.",
        suggestion: "add a deadline: #[bounded_async(Duration::from_millis(100))]",
    },

    // -----------------------------------------------------------------------
    // RS2xx: Deterministic functions
    // -----------------------------------------------------------------------
    RsDiagnostic {
        code: "RS201",
        short: "floating point type in #[deterministic] function",
        long: "f32 and f64 produce different results on different architectures \
               (x87 vs SSE, ARM vs x86). Forbidden in deterministic functions.",
        suggestion: "use FixedPoint<u128, 18> for deterministic decimal arithmetic",
    },
    RsDiagnostic {
        code: "RS202",
        short: "float cast in #[deterministic] function",
        long: "`as` casts involving floating point types have platform-dependent \
               rounding behavior.",
        suggestion: "use integer arithmetic or FixedPoint conversions",
    },
    RsDiagnostic {
        code: "RS203",
        short: "raw pointer arithmetic in #[deterministic] function",
        long: "Pointer values depend on memory layout, ASLR, and allocator \
               state — non-deterministic across different machines or runs.",
        suggestion: "use index-based access or references instead of raw pointers",
    },
    RsDiagnostic {
        code: "RS204",
        short: "system clock in #[deterministic] function",
        long: "Wall clock time varies between machines and runs. \
               std::time::Instant and std::time::SystemTime are forbidden \
               in deterministic functions.",
        suggestion: "use step counters from the cell context (self.current_step())",
    },
    RsDiagnostic {
        code: "RS205",
        short: "randomness in #[deterministic] function",
        long: "Random number generators produce different sequences on \
               different runs. All rand:: usage is forbidden in \
               deterministic functions.",
        suggestion: "use deterministic seed-based computation or remove randomness",
    },
    RsDiagnostic {
        code: "RS206",
        short: "unchecked arithmetic in #[deterministic] function",
        long: "The +, -, * operators on integers have different overflow \
               behavior in debug mode (panic) vs release mode (wrapping). \
               This is a source of non-determinism between build configurations.",
        suggestion: "use checked_add, checked_sub, or checked_mul instead",
    },
    RsDiagnostic {
        code: "RS207",
        short: "HashMap in #[deterministic] function",
        long: "HashMap uses a randomized hasher — iteration order varies \
               between runs and platforms.",
        suggestion: "use BTreeMap or BTreeSet for deterministic iteration order",
    },
    RsDiagnostic {
        code: "RS208",
        short: "inline assembly in #[deterministic] function",
        long: "Inline assembly (asm!, global_asm!) produces platform-specific \
               behavior by definition.",
        suggestion: "use portable Rust code",
    },
    RsDiagnostic {
        code: "RS209",
        short: "non-deterministic callee in #[deterministic] function",
        long: "A #[deterministic] function calls a function that is neither \
               #[deterministic] nor const fn. Determinism is transitive — \
               every callee in the call graph must also be deterministic.",
        suggestion: "mark the callee #[deterministic], make it const fn, or restructure",
    },
    RsDiagnostic {
        code: "RS210",
        short: "usize/isize in #[deterministic] function",
        long: "usize and isize have platform-dependent width (32 bits on \
               32-bit platforms, 64 bits on 64-bit platforms). A function \
               that operates on usize values may produce different results \
               on different targets.",
        suggestion: "use u32 or u64 for fixed-width integers",
    },

    // -----------------------------------------------------------------------
    // RS3xx: Addressed types
    // -----------------------------------------------------------------------
    RsDiagnostic {
        code: "RS301",
        short: "type does not implement CanonicalSerialize",
        long: "Every field in an Addressed struct must implement \
               CanonicalSerialize. Types without a canonical byte \
               representation cannot be content-addressed.",
        suggestion: "derive Addressed on the type, or implement CanonicalSerialize manually",
    },
    RsDiagnostic {
        code: "RS302",
        short: "floating point types are not canonically serializable",
        long: "f32 and f64 have multiple bit representations for the same \
               value (NaN variants, +/-0). Canonical serialization requires \
               a single byte sequence per value.",
        suggestion: "use FixedPoint<u128, 18> for deterministic decimal values",
    },
    RsDiagnostic {
        code: "RS303",
        short: "pointers cannot be addressed",
        long: "Pointer values depend on memory layout and change between \
               runs. Content addressing requires the actual data, not its \
               location.",
        suggestion: "store the data directly or use an index/identifier",
    },
    RsDiagnostic {
        code: "RS304",
        short: "HashMap has non-deterministic serialization",
        long: "HashMap iteration order is randomized. Serializing a HashMap \
               produces different byte sequences for the same logical data, \
               breaking canonical serialization.",
        suggestion: "use BTreeMap or BoundedMap (sorted array-backed)",
    },
    RsDiagnostic {
        code: "RS305",
        short: "usize/isize width is platform-dependent",
        long: "usize and isize serialize to different byte widths on \
               different platforms (4 bytes on 32-bit, 8 bytes on 64-bit). \
               Canonical serialization requires every value to produce \
               identical bytes regardless of platform.",
        suggestion: "use u32 or u64 instead of usize/isize",
    },
    RsDiagnostic {
        code: "RS306",
        short: "Addressed enum discriminant must fit in u32",
        long: "Addressed enums serialize discriminants as u32 (4 bytes, \
               little-endian). An enum with #[repr(u64)] could have \
               discriminant values exceeding u32::MAX, which would be \
               truncated during serialization.",
        suggestion: "use #[repr(u8)], #[repr(u16)], or #[repr(u32)]",
    },

    // -----------------------------------------------------------------------
    // RS4xx: Step-scoped state
    // -----------------------------------------------------------------------
    RsDiagnostic {
        code: "RS401",
        short: "#[step] state accessed outside of cell context",
        long: "#[step] state is automatically reset at step boundaries by \
               the cell runtime. Accessing it outside a cell context means \
               no runtime manages its lifecycle — the reset would never \
               happen, defeating the purpose of step scoping.",
        suggestion: "access step state from within a cell! block",
    },

    // -----------------------------------------------------------------------
    // RS5xx: Edition restrictions
    // -----------------------------------------------------------------------
    RsDiagnostic {
        code: "RS501",
        short: "heap allocation forbidden in rs edition",
        long: "Box::new() allocates on the heap. In rs edition, all \
               allocation must be bounded and explicit. Use stack values \
               or Arena<T, N> for bounded dynamic allocation.",
        suggestion: "use a stack value or Arena<T, N>",
    },
    RsDiagnostic {
        code: "RS502",
        short: "growable collections forbidden in rs edition",
        long: "Vec<T> can grow without bound via push(), triggering \
               unbounded heap allocation. In rs edition, collections \
               must have compile-time capacity.",
        suggestion: "use BoundedVec<T, N> with compile-time capacity",
    },
    RsDiagnostic {
        code: "RS503",
        short: "heap-allocated strings forbidden in rs edition",
        long: "String is a growable heap allocation. Use string slices \
               for borrowed text or ArrayString<N> for owned fixed-capacity \
               strings.",
        suggestion: "use &str or ArrayString<N>",
    },
    RsDiagnostic {
        code: "RS504",
        short: "dynamic dispatch forbidden in rs edition",
        long: "Box<dyn Trait> and &dyn Trait use vtable-based dispatch \
               with indirect calls. In rs edition, use static dispatch \
               via generics or enum dispatch for predictable performance.",
        suggestion: "use generics or enum dispatch",
    },
    RsDiagnostic {
        code: "RS505",
        short: "reference counting forbidden in rs edition",
        long: "Arc<T> and Rc<T> use heap allocation and runtime reference \
               counting. In rs edition, ownership is managed by cells \
               and channels.",
        suggestion: "use cell-owned state or bounded channels",
    },
    RsDiagnostic {
        code: "RS506",
        short: "unwinding panic forbidden in rs edition",
        long: "Stack unwinding panics add complexity to the runtime \
               (landing pads, drop handlers during unwind). In rs edition, \
               only panic = \"abort\" is permitted. This ensures stack \
               unwinding never occurs.",
        suggestion: "use Result for recoverable errors, or abort for unrecoverable",
    },
    RsDiagnostic {
        code: "RS507",
        short: "non-deterministic collections forbidden in rs edition",
        long: "HashMap and HashSet use a randomized hasher — iteration \
               order varies between runs. Forbidden in rs edition for \
               determinism.",
        suggestion: "use BTreeSet for deterministic iteration order",
    },
];

/// Look up a diagnostic by code string (e.g. "RS206").
pub fn lookup(code: &str) -> Option<&'static RsDiagnostic> {
    RS_DIAGNOSTICS.iter().find(|d| d.code == code)
}

/// Format a long explanation for `rsc --explain RSxxx`.
pub fn explain(code: &str) -> Option<String> {
    lookup(code).map(|d| {
        format!(
            "error[{}]: {}\n\n{}\n\nSuggestion: {}\n",
            d.code, d.short, d.long, d.suggestion
        )
    })
}

/// Print all error codes with short descriptions (for `rsc --list-errors`).
pub fn list_all() -> String {
    let mut out = String::with_capacity(4096);
    for d in RS_DIAGNOSTICS {
        out.push_str(&format!("{}: {}\n", d.code, d.short));
    }
    out
}

/// Register Rs error codes in rustc's diagnostic registry.
///
/// Called during compiler initialization so that rustc recognizes RS
/// error codes and can display them with `--explain`.
pub fn register_diagnostics(registry: &mut rustc_errors::registry::Registry) {
    for d in RS_DIAGNOSTICS {
        registry.register_long(d.code, d.long);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_codes_present() {
        // 33 total: RS001-RS008 (8), RS101 (1), RS201-RS210 (10),
        // RS301-RS306 (6), RS401 (1), RS501-RS507 (7)
        assert_eq!(RS_DIAGNOSTICS.len(), 33);
    }

    #[test]
    fn lookup_works() {
        assert!(lookup("RS206").is_some());
        assert!(lookup("RS999").is_none());
        assert_eq!(lookup("RS101").unwrap().code, "RS101");
    }

    #[test]
    fn codes_are_sorted() {
        for window in RS_DIAGNOSTICS.windows(2) {
            assert!(
                window[0].code < window[1].code,
                "{} should come before {}",
                window[0].code,
                window[1].code
            );
        }
    }

    #[test]
    fn no_empty_fields() {
        for d in RS_DIAGNOSTICS {
            assert!(!d.code.is_empty(), "empty code");
            assert!(!d.short.is_empty(), "empty short for {}", d.code);
            assert!(!d.long.is_empty(), "empty long for {}", d.code);
            assert!(!d.suggestion.is_empty(), "empty suggestion for {}", d.code);
        }
    }
}
