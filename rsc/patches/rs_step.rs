//! Step-scoped state enforcement: RS401.
//!
//! In rs edition, `#[step]` state must only be accessed from within a cell
//! context. Accessing step state outside a cell means no runtime manages
//! its lifecycle — reset would never happen, defeating step scoping.

use rustc_hir as hir;
use rustc_hir::def_id::{DefId, LocalDefId};
use rustc_lint::{LateContext, LateLintPass, LintContext};
use rustc_middle::ty;
use rustc_session::{declare_lint, declare_lint_pass};
use rustc_span::{Span, Symbol};

use crate::rs_edition;

declare_lint! {
    /// Step state accessed outside of cell context.
    pub RS_STEP_CONTEXT,
    Deny,
    "#[step] state accessed outside of cell context (RS401)"
}

declare_lint_pass!(StepContext => [RS_STEP_CONTEXT]);

/// Check whether a DefId has the `#[step]` attribute.
fn has_step_attr(cx: &LateContext<'_>, def_id: DefId) -> bool {
    cx.tcx.get_attrs(def_id, Symbol::intern("step"))
        .next()
        .is_some()
}

/// Check whether a function is inside a cell context. Cell-generated code
/// carries `#[__rs_cell_context]` on its enclosing impl or module.
fn is_in_cell_context(cx: &LateContext<'_>, expr_hir_id: hir::HirId) -> bool {
    // Walk up the HIR parent chain looking for a cell context marker
    let map = cx.tcx.hir();
    let mut current = expr_hir_id;

    loop {
        let parent_id = map.parent_id(current);
        if parent_id == current {
            break;
        }
        current = parent_id;

        let attrs = map.attrs(current);
        for attr in attrs {
            if let Some(ident) = attr.ident() {
                if ident.as_str() == "__rs_cell_context" {
                    return true;
                }
            }
        }
    }

    false
}

impl<'tcx> LateLintPass<'tcx> for StepContext {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx hir::Expr<'tcx>) {
        rs_edition_guard!(cx);

        // Look for path expressions that resolve to #[step] statics
        if let hir::ExprKind::Path(ref qpath) = expr.kind {
            if let Some(def_id) = resolve_qpath_def_id(cx, qpath, expr.hir_id) {
                if has_step_attr(cx, def_id) && !is_in_cell_context(cx, expr.hir_id) {
                    cx.struct_span_lint(&RS_STEP_CONTEXT, expr.span, |diag| {
                        diag.build("#[step] state accessed outside of cell context")
                            .code_str("RS401")
                            .help("#[step] state must be accessed within a cell! block")
                            .help("step reset is managed by the cell runtime")
                            .emit();
                    });
                }
            }
        }

        // Look for field access on types that contain #[step] state
        if let hir::ExprKind::Field(ref base, ref field) = expr.kind {
            let base_ty = cx.typeck_results().expr_ty_adjusted(base);
            if let ty::Adt(adt_def, _) = base_ty.peel_refs().kind() {
                // Check if the struct itself has #[step]
                if has_step_attr(cx, adt_def.did()) {
                    if !is_in_cell_context(cx, expr.hir_id) {
                        cx.struct_span_lint(&RS_STEP_CONTEXT, expr.span, |diag| {
                            diag.build("#[step] state accessed outside of cell context")
                                .code_str("RS401")
                                .help("#[step] state must be accessed within a cell! block")
                                .help("step reset is managed by the cell runtime")
                                .emit();
                        });
                    }
                }
            }
        }
    }
}

/// Resolve a QPath to a DefId if possible.
fn resolve_qpath_def_id(
    cx: &LateContext<'_>,
    qpath: &hir::QPath<'_>,
    hir_id: hir::HirId,
) -> Option<DefId> {
    let res = cx.typeck_results().qpath_res(qpath, hir_id);
    match res {
        hir::def::Res::Def(_, def_id) => Some(def_id),
        _ => None,
    }
}

/// Register the step-context lint pass in the lint store.
pub fn register_lints(store: &mut rustc_lint::LintStore) {
    store.register_lints(&[&RS_STEP_CONTEXT]);
    store.register_late_pass(|_| Box::new(StepContext));
}
