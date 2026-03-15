//! Edition restriction lint: RS504.
//!
//! Forbids dynamic dispatch (`dyn Trait`) in rs edition. Vtable-based
//! dispatch introduces indirect calls that defeat static analysis and
//! make worst-case execution time unpredictable.

use rustc_hir as hir;
use rustc_lint::{LateContext, LateLintPass, LintContext};
use rustc_session::{declare_lint, declare_lint_pass};

use crate::rs_edition;

declare_lint! {
    /// Dynamic dispatch via `dyn Trait` forbidden in rs edition.
    pub RS_NO_DYN,
    Deny,
    "dynamic dispatch forbidden in rs edition (RS504)"
}

declare_lint_pass!(NoDyn => [RS_NO_DYN]);

impl<'tcx> LateLintPass<'tcx> for NoDyn {
    fn check_ty(&mut self, cx: &LateContext<'tcx>, ty: &'tcx hir::Ty<'tcx>) {
        rs_edition_guard!(cx);

        // TraitObject represents `dyn Trait` in HIR regardless of whether
        // it appears behind &, Box, or bare (which is itself an error in
        // modern editions, but we catch it here for completeness).
        if matches!(ty.kind, hir::TyKind::TraitObject(..)) {
            cx.struct_span_lint(&RS_NO_DYN, ty.span, |diag| {
                diag.build("dynamic dispatch forbidden in rs edition")
                    .code_str("RS504")
                    .help("use generics or enum dispatch")
                    .emit();
            });
        }
    }
}

/// Register the no-dyn lint pass in the lint store.
pub fn register_lints(store: &mut rustc_lint::LintStore) {
    store.register_lints(&[&RS_NO_DYN]);
    store.register_late_pass(|_| Box::new(NoDyn));
}
