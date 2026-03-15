//! Edition restriction lint: RS504 — forbids `dyn Trait`.

use rustc_hir as hir;
use rustc_lint::{LateContext, LateLintPass, LintContext};
use rustc_session::{declare_lint, declare_lint_pass};

declare_lint! { pub RS_NO_DYN, Deny, "dynamic dispatch forbidden in rs edition (RS504)" }

declare_lint_pass!(NoDyn => [RS_NO_DYN]);

impl<'tcx> LateLintPass<'tcx> for NoDyn {
    fn check_ty(&mut self, cx: &LateContext<'tcx>, ty: &'tcx hir::Ty<'tcx, hir::AmbigArg>) {
        if matches!(ty.kind, hir::TyKind::TraitObject(..)) {
            cx.span_lint(RS_NO_DYN, ty.span, |diag| {
                diag.primary_message("dynamic dispatch forbidden in rs edition (RS504)");
                diag.help("use generics or enum dispatch");
            });
        }
    }
}

pub fn register_lints(store: &mut rustc_lint::LintStore) {
    store.register_lints(&[&RS_NO_DYN]);
    store.register_late_pass(|_| Box::new(NoDyn));
}
