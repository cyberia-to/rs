//! Deterministic function enforcement: RS201-RS210.
//!
//! MIR-level analysis for functions marked `#[deterministic]`.

use rustc_hir as hir;
use rustc_hir::def_id::{DefId, LocalDefId};
use rustc_lint::{LateContext, LateLintPass, LintContext};
use rustc_middle::mir::{self, Body, Operand, Rvalue, StatementKind, TerminatorKind};
use rustc_middle::ty::{self, Ty, TyCtxt};
use rustc_session::{declare_lint, declare_lint_pass};
use rustc_span::{Span, Symbol};

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

fn has_deterministic_attr(tcx: TyCtxt<'_>, def_id: DefId) -> bool {
    tcx.get_attrs(def_id, Symbol::intern("deterministic"))
        .next()
        .is_some()
}

fn is_deterministic_or_const(tcx: TyCtxt<'_>, def_id: DefId) -> bool {
    tcx.is_const_fn(def_id) || has_deterministic_attr(tcx, def_id)
}

fn contains_float(ty: Ty<'_>) -> bool {
    matches!(ty.kind(), ty::Float(..))
}

fn is_platform_int(ty: Ty<'_>) -> bool {
    matches!(ty.kind(), ty::Uint(ty::UintTy::Usize) | ty::Int(ty::IntTy::Isize))
}

fn is_hashmap_type(tcx: TyCtxt<'_>, ty: Ty<'_>) -> bool {
    let ty = ty.peel_refs();
    if let ty::Adt(adt_def, _) = ty.kind() {
        let path = tcx.def_path_str(adt_def.did());
        path.contains("HashMap") || path.contains("HashSet")
    } else {
        false
    }
}

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
        let fn_sig = tcx.fn_sig(def_id).skip_binder();
        for input_ty in fn_sig.inputs().skip_binder() {
            if is_platform_int(*input_ty) {
                cx.span_lint(RS_DET_USIZE, span, |diag| {
                    diag.primary_message("usize/isize in #[deterministic] function signature (RS210)");
                    diag.help("use u32 or u64 for fixed-width integers");
                });
            }
        }
        let output = fn_sig.output().skip_binder();
        if is_platform_int(output) {
            cx.span_lint(RS_DET_USIZE, span, |diag| {
                diag.primary_message("usize/isize in #[deterministic] function return type (RS210)");
                diag.help("use u32 or u64 for fixed-width integers");
            });
        }

        // MIR-level checks
        if !tcx.is_mir_available(global_def_id) {
            return;
        }

        let body = tcx.optimized_mir(global_def_id);
        check_mir_body(cx, tcx, body);
    }
}

fn check_mir_body<'tcx>(cx: &LateContext<'tcx>, tcx: TyCtxt<'tcx>, body: &Body<'tcx>) {
    for local_decl in &body.local_decls {
        let span = local_decl.source_info.span;
        if is_platform_int(local_decl.ty) {
            cx.span_lint(RS_DET_USIZE, span, |diag| {
                diag.primary_message("usize/isize in #[deterministic] function (RS210)");
                diag.help("use u32 or u64");
            });
        }
        if contains_float(local_decl.ty) {
            cx.span_lint(RS_DET_FLOAT, span, |diag| {
                diag.primary_message("f32/f64 type in #[deterministic] function (RS201)");
                diag.help("use FixedPoint<u128, 18>");
            });
        }
        if is_hashmap_type(tcx, local_decl.ty) {
            cx.span_lint(RS_DET_HASHMAP, span, |diag| {
                diag.primary_message("HashMap in #[deterministic] function (RS207)");
                diag.help("use BTreeMap");
            });
        }
    }

    for (_, block_data) in body.basic_blocks.iter_enumerated() {
        for stmt in &block_data.statements {
            let span = stmt.source_info.span;
            if let StatementKind::Assign(box (_, ref rvalue)) = stmt.kind {
                check_rvalue(cx, rvalue, span);
            }
        }

        if let Some(ref terminator) = block_data.terminator {
            let span = terminator.source_info.span;
            check_terminator(cx, tcx, terminator, span);
        }
    }
}

fn check_rvalue(cx: &LateContext<'_>, rvalue: &Rvalue<'_>, span: Span) {
    match rvalue {
        Rvalue::Cast(_, _, target_ty) => {
            if contains_float(*target_ty) {
                cx.span_lint(RS_DET_FLOAT_CAST, span, |diag| {
                    diag.primary_message("cast to/from floating point in #[deterministic] function (RS202)");
                    diag.help("float casts have platform-dependent rounding");
                });
            }
        }
        Rvalue::RawPtr(_, _) => {
            cx.span_lint(RS_DET_PTR_ARITH, span, |diag| {
                diag.primary_message("raw pointer in #[deterministic] function (RS203)");
                diag.help("memory addresses are non-deterministic");
            });
        }
        Rvalue::ThreadLocalRef(_) => {
            cx.span_lint(RS_DET_RAND, span, |diag| {
                diag.primary_message("thread-local state in #[deterministic] function (RS205)");
                diag.help("thread-local state is non-deterministic");
            });
        }
        _ => {}
    }
}

fn check_terminator<'tcx>(cx: &LateContext<'tcx>, tcx: TyCtxt<'tcx>, terminator: &mir::Terminator<'tcx>, span: Span) {
    match &terminator.kind {
        TerminatorKind::Call { func, .. } => {
            if let Some(def_id) = resolve_callee(func) {
                let path = tcx.def_path_str(def_id);

                if path == "std::time::Instant" || path == "std::time::SystemTime" {
                    cx.span_lint(RS_DET_CLOCK, span, |diag| {
                        diag.primary_message("system clock in #[deterministic] function (RS204)");
                        diag.help("use step counters");
                    });
                    return;
                }

                if path.starts_with("rand::") {
                    cx.span_lint(RS_DET_RAND, span, |diag| {
                        diag.primary_message("randomness in #[deterministic] function (RS205)");
                        diag.help("randomness is non-deterministic");
                    });
                    return;
                }

                // RS209: transitivity — skip intrinsics
                let is_intrinsic = tcx.intrinsic(def_id).is_some();
                if !is_intrinsic && !is_deterministic_or_const(tcx, def_id) {
                    let msg = format!("call to non-deterministic function {} (RS209)", path);
                    let help = format!("mark {} as #[deterministic] or const fn", path);
                    cx.span_lint(RS_DET_TRANSITIVE, span, |diag| {
                        diag.primary_message(msg.clone());
                        diag.help(help.clone());
                    });
                }
            }
        }
        TerminatorKind::InlineAsm { .. } => {
            cx.span_lint(RS_DET_ASM, span, |diag| {
                diag.primary_message("inline assembly in #[deterministic] function (RS208)");
                diag.help("assembly is platform-specific");
            });
        }
        _ => {}
    }
}

fn resolve_callee<'tcx>(func: &Operand<'tcx>) -> Option<DefId> {
    if let Operand::Constant(box constant) = func {
        if let ty::FnDef(def_id, _) = constant.const_.ty().kind() {
            return Some(*def_id);
        }
    }
    None
}

pub fn register_lints(store: &mut rustc_lint::LintStore) {
    store.register_lints(&[
        &RS_DET_FLOAT, &RS_DET_FLOAT_CAST, &RS_DET_PTR_ARITH, &RS_DET_CLOCK,
        &RS_DET_RAND, &RS_DET_UNCHECKED, &RS_DET_HASHMAP, &RS_DET_ASM,
        &RS_DET_TRANSITIVE, &RS_DET_USIZE,
    ]);
    store.register_late_pass(|_| Box::new(Deterministic));
}
