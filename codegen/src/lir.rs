//! LIR types: virtual registers, labels, and the 3-address op set used by mir2lir.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Reg(pub u32);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label(pub String);

impl Label {
    pub fn new(s: impl Into<String>) -> Self { Self(s.into()) }
}

#[derive(Debug, Clone)]
pub enum LIROp {
    // ── Control flow ──────────────────────────────────────────────────────
    Call(String),           // BL symbol
    CallIndirect(Reg),      // BLR xN  — vtable dispatch, fn-ptr, closure
    Return,
    Halt,
    Branch { cond: Reg, if_true: Label, if_false: Label },
    Jump(Label),

    // ── Program structure ─────────────────────────────────────────────────
    LabelDef(Label),
    FnStart(String),
    FnEnd,

    // ── Passthrough ───────────────────────────────────────────────────────
    Comment(String),
    Asm { lines: Vec<String> },

    // ── Data movement ─────────────────────────────────────────────────────
    LoadImm(Reg, u64),
    Move(Reg, Reg),
    LoadAddr { dst: Reg, symbol: String },

    // ── Integer arithmetic ────────────────────────────────────────────────
    Add(Reg, Reg, Reg),
    Sub(Reg, Reg, Reg),
    Mul(Reg, Reg, Reg),
    Div(Reg, Reg, Reg),     // unsigned divide
    SDiv(Reg, Reg, Reg),    // signed divide
    Rem(Reg, Reg, Reg),     // unsigned remainder (udiv + msub)
    Neg(Reg, Reg),
    Not(Reg, Reg),

    // ── Bitwise ───────────────────────────────────────────────────────────
    And(Reg, Reg, Reg),
    Or(Reg, Reg, Reg),
    Xor(Reg, Reg, Reg),
    Shl(Reg, Reg, Reg),     // logical shift left
    Shr(Reg, Reg, Reg),     // logical shift right (unsigned)
    Sar(Reg, Reg, Reg),     // arithmetic shift right (signed)

    // ── Comparisons (result is 0 or 1) ────────────────────────────────────
    Eq(Reg, Reg, Reg),
    Ne(Reg, Reg, Reg),
    Lt(Reg, Reg, Reg),      // unsigned <
    Le(Reg, Reg, Reg),      // unsigned <=
    Gt(Reg, Reg, Reg),      // unsigned >
    Ge(Reg, Reg, Reg),      // unsigned >=
    SLt(Reg, Reg, Reg),     // signed <
    SLe(Reg, Reg, Reg),     // signed <=
    SGt(Reg, Reg, Reg),     // signed >
    SGe(Reg, Reg, Reg),     // signed >=

    // ── Type conversion ───────────────────────────────────────────────────
    ZeroExt { dst: Reg, src: Reg, from_bits: u8 },  // zero-extend from N bits to 64
    SignExt { dst: Reg, src: Reg, from_bits: u8 },   // sign-extend from N bits to 64

    // ── Memory (64-bit) ───────────────────────────────────────────────────
    Load  { dst: Reg, base: Reg, offset: i32 },
    Store { src: Reg, base: Reg, offset: i32 },

    // ── Memory (sized) ────────────────────────────────────────────────────
    LoadSize  { dst: Reg, base: Reg, offset: i32, size: u8 }, // 1/2/4/8-byte zero-ext load
    StoreSize { src: Reg, base: Reg, offset: i32, size: u8 }, // 1/2/4/8-byte store

    // ── Atomics (acquire-release) ─────────────────────────────────────────
    AtomicLoad  { dst: Reg, ptr: Reg },               // LDAR
    AtomicStore { src: Reg, ptr: Reg },               // STLR
    AtomicXchg  { dst: Reg, src: Reg,  ptr: Reg },    // LDAXR/STLXR loop
    AtomicFetchAdd { dst: Reg, delta: Reg, ptr: Reg },
    AtomicFetchSub { dst: Reg, delta: Reg, ptr: Reg },
    AtomicCas  { old: Reg, new: Reg, ptr: Reg, ok: Reg },
}
