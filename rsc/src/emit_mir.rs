//! MIR serialization for the Rs → Trident pipeline.
//!
//! Walks optimized MIR for all functions in the crate and serializes
//! to the mir-format JSON format. Consumed by trident's import module.

use std::collections::BTreeMap;

use rustc_hir::def_id::{DefId, LOCAL_CRATE};
use rustc_middle::mir::{self, Body, ConstValue, Operand, Rvalue, StatementKind, TerminatorKind};
use rustc_middle::ty::{self, Ty, TyCtxt};

use mir_format::*;

/// Serialize all MIR in the current crate to JSON on stdout.
pub fn serialize_mir(tcx: TyCtxt<'_>) {
    let mut functions = Vec::new();
    let mut adts: BTreeMap<String, DefId> = BTreeMap::new();

    for &def_id in tcx.mir_keys(()) {
        let global = def_id.to_def_id();
        if !tcx.is_mir_available(global) {
            continue;
        }
        let body = tcx.optimized_mir(global);
        let name = tcx.def_path_str(global);
        functions.push(convert_function(tcx, &name, body, &mut adts));
    }

    let structs = adts
        .iter()
        .map(|(name, &did)| convert_struct(tcx, name, did))
        .collect();

    let krate = MirCrate {
        name: tcx.crate_name(LOCAL_CRATE).to_string(),
        functions,
        structs,
        constants: vec![],
    };

    let json = serde_json::to_string_pretty(&krate).expect("MIR serialization failed");
    println!("{}", json);
}

// ── Function ───────────────────────────────────────────────

fn convert_function<'tcx>(
    tcx: TyCtxt<'tcx>,
    name: &str,
    body: &Body<'tcx>,
    adts: &mut BTreeMap<String, DefId>,
) -> MirFunction {
    let locals: Vec<MirLocal> = body
        .local_decls
        .iter_enumerated()
        .map(|(local, decl)| MirLocal {
            index: local.as_u32(),
            name: None,
            ty: convert_ty(tcx, decl.ty, adts),
        })
        .collect();

    let return_ty = convert_ty(tcx, body.local_decls[mir::Local::from_u32(0)].ty, adts);

    let params: Vec<MirLocal> = (1..=body.arg_count as u32)
        .map(|i| locals[i as usize].clone())
        .collect();

    let blocks: Vec<MirBlock> = body
        .basic_blocks
        .iter_enumerated()
        .map(|(bb, data)| convert_block(tcx, bb, data, adts))
        .collect();

    MirFunction {
        name: name.to_string(),
        params,
        return_ty,
        locals,
        blocks,
    }
}

// ── Blocks ─────────────────────────────────────────────────

fn convert_block<'tcx>(
    tcx: TyCtxt<'tcx>,
    bb: mir::BasicBlock,
    data: &mir::BasicBlockData<'tcx>,
    adts: &mut BTreeMap<String, DefId>,
) -> MirBlock {
    let statements = data
        .statements
        .iter()
        .filter_map(|stmt| convert_statement(tcx, stmt, adts))
        .collect();

    let terminator = data
        .terminator
        .as_ref()
        .map(|t| convert_terminator(tcx, t, adts))
        .unwrap_or(MirTerminator::Unreachable);

    MirBlock {
        index: bb.as_u32(),
        statements,
        terminator,
    }
}

// ── Statements ─────────────────────────────────────────────

fn convert_statement<'tcx>(
    tcx: TyCtxt<'tcx>,
    stmt: &mir::Statement<'tcx>,
    adts: &mut BTreeMap<String, DefId>,
) -> Option<MirStatement> {
    match stmt.kind {
        StatementKind::Assign(box (ref place, ref rvalue)) => Some(MirStatement::Assign {
            place: convert_place(place),
            rvalue: convert_rvalue(tcx, rvalue, adts),
        }),
        _ => None,
    }
}

// ── Terminators ────────────────────────────────────────────

fn convert_terminator<'tcx>(
    tcx: TyCtxt<'tcx>,
    term: &mir::Terminator<'tcx>,
    adts: &mut BTreeMap<String, DefId>,
) -> MirTerminator {
    match &term.kind {
        TerminatorKind::Goto { target } => MirTerminator::Goto {
            target: target.as_u32(),
        },

        TerminatorKind::SwitchInt { discr, targets } => {
            let target_pairs: Vec<(u128, u32)> = targets
                .iter()
                .map(|(val, bb)| (val, bb.as_u32()))
                .collect();
            MirTerminator::SwitchInt {
                discriminant: convert_operand(tcx, discr, adts),
                targets: target_pairs,
                otherwise: targets.otherwise().as_u32(),
            }
        }

        TerminatorKind::Return => MirTerminator::Return,

        TerminatorKind::Call {
            func,
            args,
            destination,
            target,
            ..
        } => {
            let func_name = resolve_callee_name(tcx, func);
            MirTerminator::Call {
                func: func_name,
                args: args
                    .iter()
                    .map(|a| convert_operand(tcx, &a.node, adts))
                    .collect(),
                destination: convert_place(destination),
                target: target.map(|bb| bb.as_u32()),
            }
        }

        TerminatorKind::Assert {
            cond,
            expected,
            target,
            ..
        } => MirTerminator::Assert {
            cond: convert_operand(tcx, cond, adts),
            expected: *expected,
            target: target.as_u32(),
        },

        // Drop is a no-op for field elements; treat as goto to target.
        TerminatorKind::Drop { target, .. } => MirTerminator::Goto {
            target: target.as_u32(),
        },

        _ => MirTerminator::Unreachable,
    }
}

// ── Rvalues ────────────────────────────────────────────────

fn convert_rvalue<'tcx>(
    tcx: TyCtxt<'tcx>,
    rvalue: &Rvalue<'tcx>,
    adts: &mut BTreeMap<String, DefId>,
) -> MirRvalue {
    match rvalue {
        Rvalue::Use(op) => MirRvalue::Use(convert_operand(tcx, op, adts)),

        Rvalue::BinaryOp(op, box (a, b)) => MirRvalue::BinaryOp(
            convert_binop(*op),
            convert_operand(tcx, a, adts),
            convert_operand(tcx, b, adts),
        ),

        Rvalue::UnaryOp(op, a) => {
            MirRvalue::UnaryOp(convert_unop(*op), convert_operand(tcx, a, adts))
        }

        Rvalue::Cast(_, op, ty) => MirRvalue::Cast(
            MirCastKind::IntToInt,
            convert_operand(tcx, op, adts),
            convert_ty(tcx, *ty, adts),
        ),

        Rvalue::Aggregate(box kind, ops) => {
            let mir_kind = match kind {
                mir::AggregateKind::Tuple => MirAggregateKind::Tuple,
                mir::AggregateKind::Array(_) => MirAggregateKind::Array,
                mir::AggregateKind::Adt(def_id, _, _, _, _) => {
                    MirAggregateKind::Struct(tcx.def_path_str(*def_id))
                }
                _ => MirAggregateKind::Tuple,
            };
            MirRvalue::Aggregate(
                mir_kind,
                ops.iter()
                    .map(|o| convert_operand(tcx, o, adts))
                    .collect(),
            )
        }

        Rvalue::Ref(_, _, place) | Rvalue::RawPtr(_, place) => {
            MirRvalue::Ref(convert_place(place))
        }

        // Len was removed from MIR in recent rustc; arrays have compile-time lengths.

        Rvalue::Repeat(op, ct) => {
            let n = ct.try_to_target_usize(tcx).unwrap_or(0);
            MirRvalue::Repeat(convert_operand(tcx, op, adts), n)
        }

        _ => MirRvalue::Use(MirOperand::Constant(MirConstValue::Unit)),
    }
}

// ── Operands ───────────────────────────────────────────────

fn convert_operand<'tcx>(
    tcx: TyCtxt<'tcx>,
    op: &Operand<'tcx>,
    adts: &mut BTreeMap<String, DefId>,
) -> MirOperand {
    match op {
        Operand::Copy(place) => MirOperand::Copy(convert_place(place)),
        Operand::Move(place) => MirOperand::Move(convert_place(place)),
        Operand::Constant(box constant) => convert_const(tcx, constant, adts),
        _ => MirOperand::Constant(MirConstValue::Unit),
    }
}

fn convert_const<'tcx>(
    tcx: TyCtxt<'tcx>,
    constant: &mir::ConstOperand<'tcx>,
    _adts: &mut BTreeMap<String, DefId>,
) -> MirOperand {
    let ty = constant.const_.ty();

    // Try to evaluate to a scalar.
    if let mir::Const::Val(val, _) = constant.const_ {
        if let ConstValue::Scalar(scalar) = val {
            if let mir::interpret::Scalar::Int(int) = scalar {
                if ty.is_bool() {
                    let b = int.to_uint(int.size()) != 0;
                    return MirOperand::Constant(MirConstValue::Bool(b));
                }
                let v = int.to_uint(int.size());
                return MirOperand::Constant(MirConstValue::Scalar(v));
            }
        }
        if let ConstValue::ZeroSized = val {
            return MirOperand::Constant(MirConstValue::Unit);
        }
    }

    // For function items (used in Call terminators), emit Unit.
    if ty.is_fn() || ty.is_fn_ptr() {
        return MirOperand::Constant(MirConstValue::Unit);
    }

    // Try const evaluation for Ty-level constants.
    if let mir::Const::Ty(_, ct) = constant.const_ {
        if let Some(val) = ct.try_to_target_usize(tcx) {
            return MirOperand::Constant(MirConstValue::Scalar(val as u128));
        }
    }

    MirOperand::Constant(MirConstValue::Unit)
}

// ── Places ─────────────────────────────────────────────────

fn convert_place(place: &mir::Place<'_>) -> MirPlace {
    let mut result = MirPlace::Local(place.local.as_u32());
    for elem in place.projection.iter() {
        let proj = match elem {
            mir::ProjectionElem::Field(field, _) => MirProjection::Field(field.as_u32()),
            mir::ProjectionElem::Index(local) => MirProjection::Index(local.as_u32()),
            mir::ProjectionElem::ConstantIndex {
                offset, from_end, ..
            } => MirProjection::ConstantIndex {
                offset,
                from_end,
            },
            mir::ProjectionElem::Downcast(_, variant) => {
                MirProjection::Downcast(variant.as_u32())
            }
            mir::ProjectionElem::Deref => continue,
            _ => continue,
        };
        result = MirPlace::Projection {
            base: Box::new(result),
            elem: proj,
        };
    }
    result
}

// ── Types ──────────────────────────────────────────────────

fn convert_ty<'tcx>(
    tcx: TyCtxt<'tcx>,
    ty: Ty<'tcx>,
    adts: &mut BTreeMap<String, DefId>,
) -> MirType {
    match ty.kind() {
        ty::Bool => MirType::Bool,
        ty::Uint(ty::UintTy::U8) => MirType::U8,
        ty::Uint(ty::UintTy::U16) => MirType::U16,
        ty::Uint(ty::UintTy::U32) => MirType::U32,
        ty::Uint(ty::UintTy::U64) | ty::Uint(ty::UintTy::Usize) => MirType::U64,
        ty::Uint(ty::UintTy::U128) => MirType::U128,
        ty::Int(ty::IntTy::I8) => MirType::I8,
        ty::Int(ty::IntTy::I16) => MirType::I16,
        ty::Int(ty::IntTy::I32) => MirType::I32,
        ty::Int(ty::IntTy::I64) | ty::Int(ty::IntTy::Isize) => MirType::I64,
        ty::Int(ty::IntTy::I128) => MirType::I128,
        ty::Tuple(fields) if fields.is_empty() => MirType::Unit,
        ty::Tuple(fields) => {
            MirType::Tuple(fields.iter().map(|f| convert_ty(tcx, f, adts)).collect())
        }
        ty::Array(elem, len) => {
            let n = len.try_to_target_usize(tcx).unwrap_or(0);
            MirType::Array(Box::new(convert_ty(tcx, *elem, adts)), n)
        }
        ty::Adt(adt_def, _) => {
            let name = tcx.def_path_str(adt_def.did());
            adts.entry(name.clone()).or_insert(adt_def.did());
            MirType::Struct(name)
        }
        ty::Ref(_, inner, _) => MirType::Ref(Box::new(convert_ty(tcx, *inner, adts))),
        ty::Never => MirType::Unit,
        _ => MirType::Unit,
    }
}

/// Simplified type conversion for struct field definitions (no ADT tracking).
fn convert_ty_simple<'tcx>(tcx: TyCtxt<'tcx>, ty: Ty<'tcx>) -> MirType {
    let mut dummy = BTreeMap::new();
    convert_ty(tcx, ty, &mut dummy)
}

// ── Operators ──────────────────────────────────────────────

fn convert_binop(op: mir::BinOp) -> MirBinOp {
    match op {
        mir::BinOp::Add | mir::BinOp::AddUnchecked | mir::BinOp::AddWithOverflow => MirBinOp::Add,
        mir::BinOp::Sub | mir::BinOp::SubUnchecked | mir::BinOp::SubWithOverflow => MirBinOp::Sub,
        mir::BinOp::Mul | mir::BinOp::MulUnchecked | mir::BinOp::MulWithOverflow => MirBinOp::Mul,
        mir::BinOp::Div => MirBinOp::Div,
        mir::BinOp::Rem => MirBinOp::Rem,
        mir::BinOp::BitAnd => MirBinOp::BitAnd,
        mir::BinOp::BitOr => MirBinOp::BitOr,
        mir::BinOp::BitXor => MirBinOp::BitXor,
        mir::BinOp::Shl | mir::BinOp::ShlUnchecked => MirBinOp::Shl,
        mir::BinOp::Shr | mir::BinOp::ShrUnchecked => MirBinOp::Shr,
        mir::BinOp::Eq => MirBinOp::Eq,
        mir::BinOp::Ne => MirBinOp::Ne,
        mir::BinOp::Lt => MirBinOp::Lt,
        mir::BinOp::Le => MirBinOp::Le,
        mir::BinOp::Gt => MirBinOp::Gt,
        mir::BinOp::Ge => MirBinOp::Ge,
        _ => MirBinOp::Add,
    }
}

fn convert_unop(op: mir::UnOp) -> MirUnaryOp {
    match op {
        mir::UnOp::Not => MirUnaryOp::Not,
        mir::UnOp::Neg => MirUnaryOp::Neg,
        _ => MirUnaryOp::Not,
    }
}

// ── Struct collection ──────────────────────────────────────

fn convert_struct(tcx: TyCtxt<'_>, name: &str, def_id: DefId) -> MirStruct {
    let adt_def = tcx.adt_def(def_id);

    if adt_def.is_enum() {
        let variants = adt_def
            .variants()
            .iter_enumerated()
            .map(|(idx, variant)| MirVariant {
                name: variant.name.to_string(),
                discriminant: idx.as_u32(),
                fields: variant
                    .fields
                    .iter()
                    .map(|field| MirStructField {
                        name: field.name.to_string(),
                        ty: convert_ty_simple(tcx, tcx.type_of(field.did).skip_binder()),
                    })
                    .collect(),
            })
            .collect();

        MirStruct {
            name: name.to_string(),
            fields: vec![],
            variants: Some(variants),
        }
    } else {
        let variant = adt_def.non_enum_variant();
        let fields = variant
            .fields
            .iter()
            .map(|field| MirStructField {
                name: field.name.to_string(),
                ty: convert_ty_simple(tcx, tcx.type_of(field.did).skip_binder()),
            })
            .collect();

        MirStruct {
            name: name.to_string(),
            fields,
            variants: None,
        }
    }
}

// ── Helpers ────────────────────────────────────────────────

fn resolve_callee_name(tcx: TyCtxt<'_>, func: &Operand<'_>) -> String {
    if let Operand::Constant(box constant) = func {
        if let ty::FnDef(def_id, _) = constant.const_.ty().kind() {
            return tcx.def_path_str(*def_id);
        }
    }
    "unknown".to_string()
}
