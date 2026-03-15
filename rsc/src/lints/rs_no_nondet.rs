//! Edition restriction lint: RS507 — forbids HashMap/HashSet.

use rustc_hir as hir;
use rustc_lint::{LateContext, LateLintPass, LintContext};
use rustc_middle::ty;
use rustc_session::{declare_lint, declare_lint_pass};

declare_lint! { pub RS_NO_NONDET, Deny, "non-deterministic collections forbidden in rs edition (RS507)" }

declare_lint_pass!(NoNondet => [RS_NO_NONDET]);

impl<'tcx> LateLintPass<'tcx> for NoNondet {
    fn check_ty(&mut self, cx: &LateContext<'tcx>, hir_ty: &'tcx hir::Ty<'tcx, hir::AmbigArg>) {
        let Some(ty) = cx.maybe_typeck_results().and_then(|r| r.node_type_opt(hir_ty.hir_id)) else {
            return;
        };
        let ty = ty.peel_refs();
        if let ty::Adt(adt_def, _) = ty.kind() {
            let path = cx.tcx.def_path_str(adt_def.did());
            if path.contains("HashMap") || path.contains("HashSet") {
                cx.span_lint(RS_NO_NONDET, hir_ty.span, |diag| {
                    diag.primary_message("non-deterministic collections forbidden in rs edition (RS507)");
                    diag.help("use BTreeMap/BTreeSet for deterministic iteration order");
                });
            }
        }
    }
}

pub fn register_lints(store: &mut rustc_lint::LintStore) {
    store.register_lints(&[&RS_NO_NONDET]);
    store.register_late_pass(|_| Box::new(NoNondet));
}
