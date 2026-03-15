//! Step-scoped state enforcement: RS401.
//!
//! #[step] state must only be accessed from within a cell context.

use rustc_hir as hir;
use rustc_hir::def_id::DefId;
use rustc_lint::{LateContext, LateLintPass, LintContext};
use rustc_middle::ty;
use rustc_session::{declare_lint, declare_lint_pass};
use rustc_span::Symbol;

declare_lint! { pub RS_STEP_CONTEXT, Deny, "#[step] state outside cell context (RS401)" }

declare_lint_pass!(StepContext => [RS_STEP_CONTEXT]);

fn has_step_attr(cx: &LateContext<'_>, def_id: DefId) -> bool {
    cx.tcx.get_attrs(def_id, Symbol::intern("step"))
        .next()
        .is_some()
}

fn is_in_cell_context(cx: &LateContext<'_>, hir_id: hir::HirId) -> bool {
    for parent_id in cx.tcx.hir_parent_id_iter(hir_id) {
        let attrs = cx.tcx.hir_attrs(parent_id);
        for attr in attrs {
            if let Some(name) = attr.name() {
                if name.as_str() == "__rs_cell_context" {
                    return true;
                }
            }
        }
    }
    false
}

impl<'tcx> LateLintPass<'tcx> for StepContext {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx hir::Expr<'tcx>) {
        if let hir::ExprKind::Path(ref qpath) = expr.kind {
            let res = cx.typeck_results().qpath_res(qpath, expr.hir_id);
            if let hir::def::Res::Def(_, def_id) = res {
                if has_step_attr(cx, def_id) && !is_in_cell_context(cx, expr.hir_id) {
                    cx.span_lint(RS_STEP_CONTEXT, expr.span, |diag| {
                        diag.primary_message("#[step] state accessed outside cell context (RS401)");
                        diag.help("access step state from within a cell! block");
                    });
                }
            }
        }

        if let hir::ExprKind::Field(ref base, _) = expr.kind {
            let base_ty = cx.typeck_results().expr_ty_adjusted(base);
            if let ty::Adt(adt_def, _) = base_ty.peel_refs().kind() {
                if has_step_attr(cx, adt_def.did()) && !is_in_cell_context(cx, expr.hir_id) {
                    cx.span_lint(RS_STEP_CONTEXT, expr.span, |diag| {
                        diag.primary_message("#[step] state accessed outside cell context (RS401)");
                        diag.help("access step state from within a cell! block");
                    });
                }
            }
        }
    }
}

pub fn register_lints(store: &mut rustc_lint::LintStore) {
    store.register_lints(&[&RS_STEP_CONTEXT]);
    store.register_late_pass(|_| Box::new(StepContext));
}
