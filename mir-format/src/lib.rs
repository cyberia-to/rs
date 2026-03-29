//! Serialized MIR format for the Rs → Trident pipeline.
//!
//! This crate defines a stable, serde-serializable representation of Rust MIR
//! that both `rsc` (producer) and `trident` (consumer) depend on.
//! No `rustc_private` dependency — pure data types.

use serde::{Deserialize, Serialize};

// ── Top-level ──────────────────────────────────────────────

/// A serialized MIR crate: all monomorphized functions, struct layouts, constants.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MirCrate {
    pub name: String,
    pub functions: Vec<MirFunction>,
    pub structs: Vec<MirStruct>,
    pub constants: Vec<MirConst>,
}

// ── Functions ──────────────────────────────────────────────

/// A monomorphized function with its MIR body.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MirFunction {
    pub name: String,
    pub params: Vec<MirLocal>,
    pub return_ty: MirType,
    pub locals: Vec<MirLocal>,
    pub blocks: Vec<MirBlock>,
}

/// A local variable declaration (parameter or temporary).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MirLocal {
    pub index: u32,
    pub name: Option<String>,
    pub ty: MirType,
}

// ── Basic blocks ───────────────────────────────────────────

/// A basic block: linear sequence of statements, one terminator.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MirBlock {
    pub index: u32,
    pub statements: Vec<MirStatement>,
    pub terminator: MirTerminator,
}

// ── Statements ─────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum MirStatement {
    Assign { place: MirPlace, rvalue: MirRvalue },
    Nop,
}

// ── Terminators ────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum MirTerminator {
    Goto {
        target: u32,
    },
    SwitchInt {
        discriminant: MirOperand,
        /// (value, target_block) pairs.
        targets: Vec<(u128, u32)>,
        otherwise: u32,
    },
    Return,
    Call {
        func: String,
        args: Vec<MirOperand>,
        destination: MirPlace,
        /// Block to resume after the call returns.
        target: Option<u32>,
    },
    Assert {
        cond: MirOperand,
        expected: bool,
        target: u32,
    },
    Unreachable,
}

// ── Places ─────────────────────────────────────────────────

/// A memory location: a local variable optionally followed by projections.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum MirPlace {
    Local(u32),
    Projection {
        base: Box<MirPlace>,
        elem: MirProjection,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum MirProjection {
    /// Named struct field by index.
    Field(u32),
    /// Array/slice index held in a local.
    Index(u32),
    /// Compile-time constant index into an array.
    ConstantIndex { offset: u64, from_end: bool },
    /// Enum variant downcast.
    Downcast(u32),
}

// ── Rvalues ────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum MirRvalue {
    Use(MirOperand),
    BinaryOp(MirBinOp, MirOperand, MirOperand),
    UnaryOp(MirUnaryOp, MirOperand),
    CheckedBinaryOp(MirBinOp, MirOperand, MirOperand),
    Cast(MirCastKind, MirOperand, MirType),
    Aggregate(MirAggregateKind, Vec<MirOperand>),
    Ref(MirPlace),
    Len(MirPlace),
    Repeat(MirOperand, u64),
}

// ── Operands ───────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum MirOperand {
    Copy(MirPlace),
    Move(MirPlace),
    Constant(MirConstValue),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum MirConstValue {
    Scalar(u128),
    Bool(bool),
    Unit,
}

// ── Operators ──────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirBinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirUnaryOp {
    Not,
    Neg,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirCastKind {
    IntToInt,
    Truncate,
    ZeroExtend,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum MirAggregateKind {
    Tuple,
    Array,
    Struct(String),
}

// ── Types ──────────────────────────────────────────────────

/// Simplified MIR type. Rs edition guarantees all types reduce to these.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum MirType {
    Bool,
    U8,
    U16,
    U32,
    U64,
    U128,
    I8,
    I16,
    I32,
    I64,
    I128,
    Unit,
    Tuple(Vec<MirType>),
    Array(Box<MirType>, u64),
    Struct(String),
    Ref(Box<MirType>),
}

// ── Structs ────────────────────────────────────────────────

/// Layout of a user-defined struct (ADT).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MirStruct {
    pub name: String,
    pub fields: Vec<MirStructField>,
    /// Enum variants, if this is an enum.
    pub variants: Option<Vec<MirVariant>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MirStructField {
    pub name: String,
    pub ty: MirType,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MirVariant {
    pub name: String,
    pub discriminant: u32,
    pub fields: Vec<MirStructField>,
}

// ── Constants ──────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MirConst {
    pub name: String,
    pub ty: MirType,
    pub value: u128,
}

// ── Serialization helpers ──────────────────────────────────

impl MirCrate {
    /// Serialize to JSON bytes.
    pub fn to_json(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec_pretty(self)
    }

    /// Deserialize from JSON bytes.
    pub fn from_json(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }
}

// ── Tests ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_minimal_crate() {
        let krate = MirCrate {
            name: "test_crate".into(),
            functions: vec![MirFunction {
                name: "add".into(),
                params: vec![
                    MirLocal { index: 1, name: Some("a".into()), ty: MirType::U32 },
                    MirLocal { index: 2, name: Some("b".into()), ty: MirType::U32 },
                ],
                return_ty: MirType::U32,
                locals: vec![
                    MirLocal { index: 0, name: None, ty: MirType::U32 },
                    MirLocal { index: 1, name: Some("a".into()), ty: MirType::U32 },
                    MirLocal { index: 2, name: Some("b".into()), ty: MirType::U32 },
                ],
                blocks: vec![MirBlock {
                    index: 0,
                    statements: vec![MirStatement::Assign {
                        place: MirPlace::Local(0),
                        rvalue: MirRvalue::BinaryOp(
                            MirBinOp::Add,
                            MirOperand::Copy(MirPlace::Local(1)),
                            MirOperand::Copy(MirPlace::Local(2)),
                        ),
                    }],
                    terminator: MirTerminator::Return,
                }],
            }],
            structs: vec![],
            constants: vec![],
        };

        let json = serde_json::to_string_pretty(&krate).unwrap();
        let restored: MirCrate = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.name, "test_crate");
        assert_eq!(restored.functions.len(), 1);
        assert_eq!(restored.functions[0].name, "add");
        assert_eq!(restored.functions[0].blocks.len(), 1);
    }

    #[test]
    fn round_trip_control_flow() {
        let krate = MirCrate {
            name: "cf_test".into(),
            functions: vec![MirFunction {
                name: "max".into(),
                params: vec![
                    MirLocal { index: 1, name: Some("a".into()), ty: MirType::U32 },
                    MirLocal { index: 2, name: Some("b".into()), ty: MirType::U32 },
                ],
                return_ty: MirType::U32,
                locals: vec![
                    MirLocal { index: 0, name: None, ty: MirType::U32 },
                    MirLocal { index: 1, name: Some("a".into()), ty: MirType::U32 },
                    MirLocal { index: 2, name: Some("b".into()), ty: MirType::U32 },
                    MirLocal { index: 3, name: None, ty: MirType::Bool },
                ],
                blocks: vec![
                    MirBlock {
                        index: 0,
                        statements: vec![MirStatement::Assign {
                            place: MirPlace::Local(3),
                            rvalue: MirRvalue::BinaryOp(
                                MirBinOp::Gt,
                                MirOperand::Copy(MirPlace::Local(1)),
                                MirOperand::Copy(MirPlace::Local(2)),
                            ),
                        }],
                        terminator: MirTerminator::SwitchInt {
                            discriminant: MirOperand::Copy(MirPlace::Local(3)),
                            targets: vec![(0, 2)],
                            otherwise: 1,
                        },
                    },
                    MirBlock {
                        index: 1,
                        statements: vec![MirStatement::Assign {
                            place: MirPlace::Local(0),
                            rvalue: MirRvalue::Use(MirOperand::Copy(MirPlace::Local(1))),
                        }],
                        terminator: MirTerminator::Goto { target: 3 },
                    },
                    MirBlock {
                        index: 2,
                        statements: vec![MirStatement::Assign {
                            place: MirPlace::Local(0),
                            rvalue: MirRvalue::Use(MirOperand::Copy(MirPlace::Local(2))),
                        }],
                        terminator: MirTerminator::Goto { target: 3 },
                    },
                    MirBlock {
                        index: 3,
                        statements: vec![],
                        terminator: MirTerminator::Return,
                    },
                ],
            }],
            structs: vec![],
            constants: vec![],
        };

        let json = serde_json::to_string(&krate).unwrap();
        let restored: MirCrate = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.functions[0].blocks.len(), 4);
        match &restored.functions[0].blocks[0].terminator {
            MirTerminator::SwitchInt { targets, otherwise, .. } => {
                assert_eq!(targets.len(), 1);
                assert_eq!(*otherwise, 1);
            }
            other => panic!("expected SwitchInt, got {:?}", other),
        }
    }

    #[test]
    fn round_trip_struct_and_projection() {
        let krate = MirCrate {
            name: "struct_test".into(),
            functions: vec![MirFunction {
                name: "get_x".into(),
                params: vec![MirLocal {
                    index: 1,
                    name: Some("p".into()),
                    ty: MirType::Struct("Point".into()),
                }],
                return_ty: MirType::U32,
                locals: vec![
                    MirLocal { index: 0, name: None, ty: MirType::U32 },
                    MirLocal { index: 1, name: Some("p".into()), ty: MirType::Struct("Point".into()) },
                ],
                blocks: vec![MirBlock {
                    index: 0,
                    statements: vec![MirStatement::Assign {
                        place: MirPlace::Local(0),
                        rvalue: MirRvalue::Use(MirOperand::Copy(MirPlace::Projection {
                            base: Box::new(MirPlace::Local(1)),
                            elem: MirProjection::Field(0),
                        })),
                    }],
                    terminator: MirTerminator::Return,
                }],
            }],
            structs: vec![MirStruct {
                name: "Point".into(),
                fields: vec![
                    MirStructField { name: "x".into(), ty: MirType::U32 },
                    MirStructField { name: "y".into(), ty: MirType::U32 },
                ],
                variants: None,
            }],
            constants: vec![],
        };

        let json = serde_json::to_string(&krate).unwrap();
        let restored: MirCrate = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.structs.len(), 1);
        assert_eq!(restored.structs[0].fields.len(), 2);
        match &restored.functions[0].blocks[0].statements[0] {
            MirStatement::Assign { rvalue: MirRvalue::Use(MirOperand::Copy(place)), .. } => {
                match place {
                    MirPlace::Projection { elem: MirProjection::Field(0), .. } => {}
                    other => panic!("expected Field(0), got {:?}", other),
                }
            }
            other => panic!("expected Assign/Use/Copy, got {:?}", other),
        }
    }

    #[test]
    fn round_trip_all_binops() {
        let ops = [
            MirBinOp::Add, MirBinOp::Sub, MirBinOp::Mul, MirBinOp::Div,
            MirBinOp::Rem, MirBinOp::BitAnd, MirBinOp::BitOr, MirBinOp::BitXor,
            MirBinOp::Shl, MirBinOp::Shr, MirBinOp::Eq, MirBinOp::Ne,
            MirBinOp::Lt, MirBinOp::Le, MirBinOp::Gt, MirBinOp::Ge,
        ];
        for op in &ops {
            let json = serde_json::to_string(op).unwrap();
            let restored: MirBinOp = serde_json::from_str(&json).unwrap();
            assert_eq!(*op, restored);
        }
    }

    #[test]
    fn round_trip_all_types() {
        let types = vec![
            MirType::Bool, MirType::U8, MirType::U16, MirType::U32,
            MirType::U64, MirType::U128, MirType::I8, MirType::I16,
            MirType::I32, MirType::I64, MirType::I128, MirType::Unit,
            MirType::Tuple(vec![MirType::U32, MirType::Bool]),
            MirType::Array(Box::new(MirType::U8), 32),
            MirType::Struct("Foo".into()),
            MirType::Ref(Box::new(MirType::U64)),
        ];
        for ty in &types {
            let json = serde_json::to_string(ty).unwrap();
            let restored: MirType = serde_json::from_str(&json).unwrap();
            assert_eq!(*ty, restored);
        }
    }
}
