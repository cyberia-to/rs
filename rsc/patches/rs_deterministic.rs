//! Deterministic function enforcement: RS201–RS210.
//!
//! MIR-level analysis for functions marked `#[deterministic]`. The proc-macro
//! catches token-level violations (RS201–RS205, RS207, RS208); this lint pass
//! adds what only MIR analysis can provide:
//!
//! - RS206: unchecked arithmetic operators (+, -, * on integers)
//! - RS209: transitivity — every callee must be #[deterministic] or const fn
//! - RS210: usize/isize in signatures and local bindings
//!
//! The remaining codes (RS201–RS205, RS207, RS208) are also checked here as a
//! second enforcement layer — catching indirection the proc-macro misses.

use rustc_hir as hir;
use rustc_hir::def_id::{DefId, LocalDefId};
use rustc_lint::{LateContext, LateLintPass, LintContext};
use rustc_middle::mir::{self, Body, Operand, Rvalue, StatementKind, TerminatorKind};
use rustc_middle::ty::{self, Ty, TyCtxt};
use rustc_session::{declare_lint, declare_lint_pass};
use rustc_span::Span;

// ---------------------------------------------------------------------------
// Lint declarations — one per error code
// ---------------------------------------------------------------------------

declare_lint! { pub RS_DET_FLOAT, Deny, "f32/f64 in #[deterministic] function (RS201)" }
declare_lint! { pub RS_DET_FLOAT_CAST, Deny, "float cast in #[deterministic] function (RS202)" }
declare_lint! { pub RS_DET_PTR_ARITH, Deny, "raw pointer arithmetic in #[deterministic] function (RS203)" }
declare_lint! { pub RS_DET_CLOCK, Deny, "system clock in #[deterministic] function (RS204)" }
declare_lint! { pub RS_DET_RAND, Deny, "randomness in #[deterministic] function (RS205)" }
declare_lint! { pub RS_DET_UNCHECKED, Deny, "unchecked arithmetic in #[deterministic] function (RS206)" }
declare_lint! { pub RS_DET_HASHMAP, Deny, "HashMap in #[deterministic] function (RS207)" }
declare_lint! { pub RS_DET_ASM, Deny, "inline assembly in #[deterministic] function (RS208)" }
declare_lint! { pub RS_DET_TRANSITIVE, Deny, "non-deterministic callee in #[deterministic] function (RS209)" }
declare_lint! { pub RS_DET_USIZE, Deny, "usize/isize in #[deterministic] function (RS210)" }

declare_lint_pass!(Deterministic => [
    RS_DET_FLOAT, RS_DET_FLOAT_CAST, RS_DET_PTR_ARITH, RS_DET_CLOCK,
    RS_DET_RAND, RS_DET_UNCHECKED, RS_DET_HASHMAP, RS_DET_ASM,
    RS_DET_TRANSITIVE, RS_DET_USIZE,
]);

// ---------------------------------------------------------------------------
// Attribute detection
// ---------------------------------------------------------------------------

/// Returns true if the function has `#[deterministic]` attribute.
fn has_deterministic_attr(tcx: TyCtxt<'_>, def_id: DefId) -> bool {
    tcx.get_attrs(def_id, rustc_span::Symbol::intern("deterministic"))
        .next()
        .is_some()
}

/// Returns true if the function is const or has `#[deterministic]`.
fn is_deterministic_or_const(tcx: TyCtxt<'_>, def_id: DefId) -> bool {
    tcx.is_const_fn(def_id) || has_deterministic_attr(tcx, def_id)
}

// ---------------------------------------------------------------------------
// Type checks
// ---------------------------------------------------------------------------

/// Checks whether a type is or contains f32/f64.
fn contains_float(ty: Ty<'_>) -> bool {
    matches!(ty.kind(), ty::Float(..))
}

/// Checks whether a type is usize or isize.
fn is_platform_int(ty: Ty<'_>) -> bool {
    matches!(ty.kind(), ty::Uint(ty::UintTy::Usize) | ty::Int(ty::IntTy::Isize))
}

/// Checks whether a type is a raw pointer.
fn is_raw_pointer(ty: Ty<'_>) -> bool {
    ty.is_unsafe_ptr()
}

/// Checks whether a type resolves to HashMap or HashSet.
fn is_hashmap_type(tcx: TyCtxt<'_>, ty: Ty<'_>) -> bool {
    let ty = ty.peel_refs();
    if let ty::Adt(adt_def, _) = ty.kind() {
        let path = tcx.def_path_str(adt_def.did());
        path.contains("HashMap") || path.contains("HashSet")
    } else {
        false
    }
}

/// Checks whether a DefId refers to a known clock type (std::time::Instant).
fn is_clock_type(tcx: TyCtxt<'_>, def_id: DefId) -> bool {
    let path = tcx.def_path_str(def_id);
    path == "std::time::Instant" || path == "std::time::SystemTime"
}

/// Checks whether a DefId refers to rand crate functions.
fn is_rand_fn(tcx: TyCtxt<'_>, def_id: DefId) -> bool {
    let path = tcx.def_path_str(def_id);
    path.starts_with("rand::")
}

// ---------------------------------------------------------------------------
// MIR analysis
// ---------------------------------------------------------------------------

impl<'tcx> LateLintPass<'tcx> for Deterministic {
    fn check_fn(
        &mut self,
        cx: &LateContext<'tcx>,
        _kind: hir::intravisit::FnKind<'tcx>,
        _decl: &'tcx hir::FnDecl<'tcx>,
        _body: &'tcx hir::Body<'tcx>,
        span: Span,
        def_id: LocalDefId,
    ) {
        let tcx = cx.tcx;
        let global_def_id = def_id.to_def_id();

        if !has_deterministic_attr(tcx, global_def_id) {
            return;
        }

        // RS210: check signature for usize/isize
        check_signature_types(cx, def_id, span);

        // MIR-level checks require optimized MIR to be available
        if !tcx.is_mir_available(global_def_id) {
            return;
        }

        let body = tcx.optimized_mir(global_def_id);
        check_mir_body(cx, body, span);
    }
}

/// RS210: flag usize/isize in function parameter and return types.
fn check_signature_types(cx: &LateContext<'_>, def_id: LocalDefId, fn_span: Span) {
    let tcx = cx.tcx;
    let fn_sig = tcx.fn_sig(def_id).skip_binder();

    for input_ty in fn_sig.inputs().skip_binder() {
        if is_platform_int(*input_ty) {
            emit_lint(cx, &RS_DET_USIZE, fn_span, "RS210",
                "usize/isize in #[deterministic] function signature",
                "usize is 32 bits on 32-bit platforms and 64 bits on 64-bit platforms; use u32 or u64");
        }
    }

    let output = fn_sig.output().skip_binder();
    if is_platform_int(output) {
        emit_lint(cx, &RS_DET_USIZE, fn_span, "RS210",
            "usize/isize in #[deterministic] function return type",
            "use u32 or u64 for deterministic width");
    }
}

/// Walk the MIR body checking statements and terminators.
fn check_mir_body<'tcx>(cx: &LateContext<'tcx>, body: &Body<'tcx>, fn_span: Span) {
    let tcx = cx.tcx;

    for (_, block_data) in body.basic_blocks.iter_enumerated() {
        // Check statements
        for stmt in &block_data.statements {
            let span = stmt.source_info.span;

            if let StatementKind::Assign(box (_, ref rvalue)) = stmt.kind {
                check_rvalue(cx, rvalue, span, fn_span);
            }
        }

        // Check terminators
        if let Some(ref terminator) = block_data.terminator {
            let span = terminator.source_info.span;
            check_terminator(cx, tcx, terminator, span, fn_span);
        }
    }

    // Check local declarations for usize/isize types (RS210)
    for local_decl in &body.local_decls {
        if is_platform_int(local_decl.ty) {
            let span = local_decl.source_info.span;
            emit_lint(cx, &RS_DET_USIZE, span, "RS210",
                "usize/isize used in #[deterministic] function",
                "usize is 32 bits on 32-bit platforms and 64 bits on 64-bit platforms; use u32 or u64");
        }

        // RS201: float types in locals
        if contains_float(local_decl.ty) {
            let span = local_decl.source_info.span;
            emit_lint(cx, &RS_DET_FLOAT, span, "RS201",
                "f32/f64 type used in #[deterministic] function",
                "use FixedPoint<u128, 18> for deterministic decimal arithmetic");
        }

        // RS207: HashMap in locals
        if is_hashmap_type(cx.tcx, local_decl.ty) {
            let span = local_decl.source_info.span;
            emit_lint(cx, &RS_DET_HASHMAP, span, "RS207",
                "HashMap used in #[deterministic] function",
                "HashMap iteration order is non-deterministic; use BTreeMap");
        }
    }
}

/// Check an rvalue for determinism violations.
fn check_rvalue(cx: &LateContext<'_>, rvalue: &Rvalue<'_>, span: Span, _fn_span: Span) {
    match rvalue {
        // RS206: unchecked arithmetic (Add, Sub, Mul on integers)
        Rvalue::BinaryOp(op, box (ref lhs, _)) | Rvalue::CheckedBinaryOp(op, box (ref lhs, _)) => {
            if matches!(op, mir::BinOp::Add | mir::BinOp::Sub | mir::BinOp::Mul) {
                // CheckedBinaryOp is fine — it's the checked variant.
                // Only flag plain BinaryOp on integer types.
                if matches!(rvalue, Rvalue::BinaryOp(..)) {
                    let lhs_ty = lhs.ty(&cx.tcx.body_for_def_id_default_body(
                        // Use local_decls from MIR body — accessed via context
                    ).unwrap().local_decls, cx.tcx);

                    if lhs_ty.is_integral() {
                        let op_name = match op {
                            mir::BinOp::Add => "addition",
                            mir::BinOp::Sub => "subtraction",
                            mir::BinOp::Mul => "multiplication",
                            _ => "arithmetic",
                        };
                        let help = match op {
                            mir::BinOp::Add => "use checked_add instead of +",
                            mir::BinOp::Sub => "use checked_sub instead of -",
                            mir::BinOp::Mul => "use checked_mul instead of *",
                            _ => "use checked arithmetic methods",
                        };
                        emit_lint(cx, &RS_DET_UNCHECKED, span, "RS206",
                            &format!("unchecked {} in #[deterministic] function", op_name),
                            help);
                    }
                }
            }
        }

        // RS202: float casts
        Rvalue::Cast(_, ref operand, target_ty) => {
            if contains_float(*target_ty) {
                emit_lint(cx, &RS_DET_FLOAT_CAST, span, "RS202",
                    "cast to floating point in #[deterministic] function",
                    "rounding behavior of float casts is platform-dependent");
            }
            // Also check source type
            if let Some(src_ty) = operand_ty(operand, cx) {
                if contains_float(src_ty) {
                    emit_lint(cx, &RS_DET_FLOAT_CAST, span, "RS202",
                        "cast from floating point in #[deterministic] function",
                        "rounding behavior of float casts is platform-dependent");
                }
            }
        }

        // RS203: raw pointer arithmetic
        Rvalue::AddressOf(_, _) => {
            emit_lint(cx, &RS_DET_PTR_ARITH, span, "RS203",
                "raw pointer arithmetic in #[deterministic] function",
                "memory addresses are non-deterministic across runs");
        }

        // RS208: inline assembly
        Rvalue::ThreadLocalRef(_) => {
            // Thread-locals are non-deterministic state
            emit_lint(cx, &RS_DET_RAND, span, "RS205",
                "thread-local state in #[deterministic] function",
                "thread-local state is non-deterministic");
        }

        _ => {}
    }
}

/// Check a terminator for determinism violations.
fn check_terminator<'tcx>(
    cx: &LateContext<'tcx>,
    tcx: TyCtxt<'tcx>,
    terminator: &mir::Terminator<'tcx>,
    span: Span,
    _fn_span: Span,
) {
    match &terminator.kind {
        // RS209: transitivity — every callee must be deterministic or const
        TerminatorKind::Call { func, .. } => {
            if let Some(def_id) = resolve_callee(tcx, func) {
                // Allow intrinsics and lang items (primitive operations)
                if tcx.is_intrinsic(def_id) {
                    return;
                }

                // RS204: clock usage
                if is_clock_type(tcx, def_id) {
                    emit_lint(cx, &RS_DET_CLOCK, span, "RS204",
                        "std::time::Instant used in #[deterministic] function",
                        "wall clock time is non-deterministic; use step counters");
                    return;
                }

                // RS205: randomness
                if is_rand_fn(tcx, def_id) {
                    emit_lint(cx, &RS_DET_RAND, span, "RS205",
                        "randomness used in #[deterministic] function",
                        "randomness is non-deterministic by definition");
                    return;
                }

                // RS209: transitivity check
                if !is_deterministic_or_const(tcx, def_id) {
                    let callee_name = tcx.def_path_str(def_id);
                    emit_lint(cx, &RS_DET_TRANSITIVE, span, "RS209",
                        &format!(
                            "call to non-deterministic function {}() in #[deterministic] function",
                            callee_name
                        ),
                        &format!("mark {}() as #[deterministic] or const fn", callee_name));
                }
            }
        }

        // RS208: inline assembly
        TerminatorKind::InlineAsm { .. } => {
            emit_lint(cx, &RS_DET_ASM, span, "RS208",
                "inline assembly in #[deterministic] function",
                "assembly is platform-specific by definition");
        }

        _ => {}
    }
}

/// Resolve a MIR call operand to a DefId.
fn resolve_callee<'tcx>(tcx: TyCtxt<'tcx>, func: &Operand<'tcx>) -> Option<DefId> {
    match func {
        Operand::Constant(box constant) => {
            if let ty::FnDef(def_id, _) = constant.const_.ty().kind() {
                Some(*def_id)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Get the type of an operand if available.
fn operand_ty<'tcx>(operand: &Operand<'tcx>, _cx: &LateContext<'tcx>) -> Option<Ty<'tcx>> {
    match operand {
        Operand::Constant(box constant) => Some(constant.const_.ty()),
        _ => None,
    }
}

/// Emit a lint diagnostic with error code and help text.
fn emit_lint(
    cx: &LateContext<'_>,
    lint: &'static rustc_lint::Lint,
    span: Span,
    code: &str,
    message: &str,
    help: &str,
) {
    cx.struct_span_lint(lint, span, |diag| {
        let mut builder = diag.build(message);
        builder.code_str(code);
        if !help.is_empty() {
            builder.help(help);
        }
        builder.emit();
    });
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

pub fn register_lints(store: &mut rustc_lint::LintStore) {
    store.register_lints(&[
        &RS_DET_FLOAT, &RS_DET_FLOAT_CAST, &RS_DET_PTR_ARITH, &RS_DET_CLOCK,
        &RS_DET_RAND, &RS_DET_UNCHECKED, &RS_DET_HASHMAP, &RS_DET_ASM,
        &RS_DET_TRANSITIVE, &RS_DET_USIZE,
    ]);
    store.register_late_pass(|_| Box::new(Deterministic));
}
