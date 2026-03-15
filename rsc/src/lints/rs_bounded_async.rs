//! Bounded async enforcement: RS101.
//!
//! Every async function must have an explicit deadline.

use rustc_hir as hir;
use rustc_hir::def_id::LocalDefId;
use rustc_lint::{LateContext, LateLintPass, LintContext};
use rustc_session::{declare_lint, declare_lint_pass};
use rustc_span::Span;

declare_lint! { pub RS_UNBOUNDED_ASYNC, Deny, "async functions must have a deadline (RS101)" }

declare_lint_pass!(BoundedAsync => [RS_UNBOUNDED_ASYNC]);

fn has_deadline_attr(cx: &LateContext<'_>, def_id: LocalDefId) -> bool {
    let hir_id = cx.tcx.local_def_id_to_hir_id(def_id);
    let attrs = cx.tcx.hir_attrs(hir_id);
    for attr in attrs {
        if let Some(ident) = attr.name() {
            let name = ident.as_str();
            if name == "bounded_async" || name == "rs_bounded_async" || name == "__rs_cell_context" {
                return true;
            }
        }
    }
    false
}

impl<'tcx> LateLintPass<'tcx> for BoundedAsync {
    fn check_fn(
        &mut self,
        cx: &LateContext<'tcx>,
        kind: hir::intravisit::FnKind<'tcx>,
        _decl: &'tcx hir::FnDecl<'tcx>,
        _body: &'tcx hir::Body<'tcx>,
        span: Span,
        def_id: LocalDefId,
    ) {
        let is_async = match kind {
            hir::intravisit::FnKind::ItemFn(_, _, header) => header.asyncness.is_async(),
            hir::intravisit::FnKind::Method(_, sig) => sig.header.asyncness.is_async(),
            hir::intravisit::FnKind::Closure => return,
        };

        if !is_async || has_deadline_attr(cx, def_id) {
            return;
        }

        cx.span_lint(RS_UNBOUNDED_ASYNC, span, |diag| {
            diag.primary_message("async functions must have a deadline (RS101)");
            diag.help("add #[bounded_async(Duration::from_millis(100))]");
        });
    }
}

pub fn register_lints(store: &mut rustc_lint::LintStore) {
    store.register_lints(&[&RS_UNBOUNDED_ASYNC]);
    store.register_late_pass(|_| Box::new(BoundedAsync));
}
