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
    RsDiagnostic { code: "RS001", short: "read from write-only register", long: "Write-only registers (access = \"wo\") have no read() method.", suggestion: "use a read-write or read-only register" },
    RsDiagnostic { code: "RS002", short: "write to read-only register", long: "Read-only registers (access = \"ro\") have no write() method.", suggestion: "use a read-write or write-only register" },
    RsDiagnostic { code: "RS003", short: "field exceeds register width", long: "A field's bit range extends beyond the register's declared width.", suggestion: "adjust the field's bit range or increase the register width" },
    RsDiagnostic { code: "RS004", short: "field value exceeds bit range", long: "A constant value is too large for the field's bit width.", suggestion: "use a value that fits within the field width" },
    RsDiagnostic { code: "RS005", short: "overlapping field bits", long: "Two fields declare overlapping bit ranges.", suggestion: "adjust bit ranges so fields don't overlap" },
    RsDiagnostic { code: "RS006", short: "enum variant exceeds field width", long: "An enum has more variants than the field can represent.", suggestion: "remove variants or widen the field" },
    RsDiagnostic { code: "RS007", short: "address outside declared bank", long: "A register's offset is outside the declared bank_size.", suggestion: "use an offset within the bank size" },
    RsDiagnostic { code: "RS008", short: "enum does not cover all bit patterns", long: "An enum does not cover all possible bit patterns for the field width.", suggestion: "add a Reserved variant" },
    RsDiagnostic { code: "RS101", short: "async function must have a deadline", long: "Every async function must have an explicit deadline in rs edition.", suggestion: "#[bounded_async(Duration::from_millis(100))]" },
    RsDiagnostic { code: "RS201", short: "floating point in #[deterministic] function", long: "f32/f64 produce different results on different architectures.", suggestion: "use FixedPoint<u128, 18>" },
    RsDiagnostic { code: "RS202", short: "float cast in #[deterministic] function", long: "Float casts have platform-dependent rounding.", suggestion: "use integer arithmetic or FixedPoint" },
    RsDiagnostic { code: "RS203", short: "raw pointer arithmetic in #[deterministic] function", long: "Pointer values are non-deterministic across runs.", suggestion: "use index-based access" },
    RsDiagnostic { code: "RS204", short: "system clock in #[deterministic] function", long: "Wall clock time varies between machines and runs.", suggestion: "use step counters" },
    RsDiagnostic { code: "RS205", short: "randomness in #[deterministic] function", long: "Random number generators produce different sequences.", suggestion: "use deterministic seed-based computation" },
    RsDiagnostic { code: "RS206", short: "unchecked arithmetic in #[deterministic] function", long: "Overflow behavior differs between debug and release.", suggestion: "use checked_add, checked_sub, checked_mul" },
    RsDiagnostic { code: "RS207", short: "HashMap in #[deterministic] function", long: "HashMap iteration order is non-deterministic.", suggestion: "use BTreeMap" },
    RsDiagnostic { code: "RS208", short: "inline assembly in #[deterministic] function", long: "Assembly is platform-specific by definition.", suggestion: "use portable Rust code" },
    RsDiagnostic { code: "RS209", short: "non-deterministic callee in #[deterministic] function", long: "Determinism is transitive \u{2014} every callee must be deterministic.", suggestion: "mark callee #[deterministic] or const fn" },
    RsDiagnostic { code: "RS210", short: "usize/isize in #[deterministic] function", long: "usize/isize have platform-dependent width.", suggestion: "use u32 or u64" },
    RsDiagnostic { code: "RS301", short: "type does not implement CanonicalSerialize", long: "Every field in an Addressed struct must be serializable.", suggestion: "derive Addressed or implement CanonicalSerialize" },
    RsDiagnostic { code: "RS302", short: "float in Addressed type", long: "f32/f64 have multiple bit representations for the same value.", suggestion: "use FixedPoint" },
    RsDiagnostic { code: "RS303", short: "pointer in Addressed type", long: "Pointer values change between runs.", suggestion: "store data directly" },
    RsDiagnostic { code: "RS304", short: "HashMap in Addressed type", long: "HashMap serialization order is non-deterministic.", suggestion: "use BTreeMap or BoundedMap" },
    RsDiagnostic { code: "RS305", short: "usize/isize in Addressed type", long: "usize/isize serialize to different widths per platform.", suggestion: "use u32 or u64" },
    RsDiagnostic { code: "RS306", short: "enum discriminant exceeds u32", long: "Addressed enums serialize discriminants as u32.", suggestion: "use #[repr(u8/u16/u32)]" },
    RsDiagnostic { code: "RS401", short: "#[step] state outside cell context", long: "Step state must be managed by a cell runtime.", suggestion: "access from within a cell! block" },
    RsDiagnostic { code: "RS501", short: "heap allocation forbidden", long: "Box::new() allocates on the heap.", suggestion: "use Arena<T, N>" },
    RsDiagnostic { code: "RS502", short: "growable collections forbidden", long: "Vec<T> can grow without bound.", suggestion: "use BoundedVec<T, N>" },
    RsDiagnostic { code: "RS503", short: "heap strings forbidden", long: "String is a growable heap allocation.", suggestion: "use &str or ArrayString<N>" },
    RsDiagnostic { code: "RS504", short: "dynamic dispatch forbidden", long: "dyn Trait uses vtable-based indirect calls.", suggestion: "use generics or enum dispatch" },
    RsDiagnostic { code: "RS505", short: "reference counting forbidden", long: "Arc/Rc use heap allocation and runtime counting.", suggestion: "use cell-owned state" },
    RsDiagnostic { code: "RS506", short: "unwinding panic forbidden", long: "Stack unwinding adds unnecessary complexity.", suggestion: "set panic = \"abort\"" },
    RsDiagnostic { code: "RS507", short: "non-deterministic collections forbidden", long: "HashMap/HashSet have non-deterministic iteration.", suggestion: "use BTreeSet" },
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
