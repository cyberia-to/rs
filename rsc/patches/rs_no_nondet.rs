//! Edition restriction lint: RS507.
//!
//! Forbids non-deterministic collections (`HashMap`, `HashSet`) in rs
//! edition. Their randomized hasher means iteration order varies between
//! runs — a source of non-determinism in consensus-critical code.

use rustc_hir as hir;
use rustc_hir::def_id::DefId;
use rustc_lint::{LateContext, LateLintPass, LintContext};
use rustc_middle::ty::{self, Ty};
use rustc_session::{declare_lint, declare_lint_pass};

use crate::rs_edition;

declare_lint! {
    /// Non-deterministic collections forbidden in rs edition.
    pub RS_NO_NONDET,
    Deny,
    "non-deterministic collections forbidden in rs edition (RS507)"
}

declare_lint_pass!(NoNondet => [RS_NO_NONDET]);

/// Type paths for non-deterministic collections.
const NONDET_TYPES: &[&[&str]] = &[
    &["std", "collections", "hash", "map", "HashMap"],
    &["std", "collections", "hash", "set", "HashSet"],
    &["hashbrown", "map", "HashMap"],
    &["hashbrown", "set", "HashSet"],
];

fn is_nondet_type(cx: &LateContext<'_>, def_id: DefId) -> bool {
    let path = cx.tcx.def_path_str(def_id);
    NONDET_TYPES.iter().any(|segments| {
        let expected = segments.join("::");
        path == expected
    })
}

impl<'tcx> LateLintPass<'tcx> for NoNondet {
    fn check_ty(&mut self, cx: &LateContext<'tcx>, hir_ty: &'tcx hir::Ty<'tcx>) {
        rs_edition_guard!(cx);

        let Some(ty) = cx.typeck_results().node_type_opt(hir_ty.hir_id) else {
            return;
        };

        check_for_nondet(cx, ty, hir_ty.span);
    }

    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx hir::Expr<'tcx>) {
        rs_edition_guard!(cx);

        let ty = cx.typeck_results().expr_ty_adjusted(expr);
        check_for_nondet(cx, ty, expr.span);
    }
}

fn check_for_nondet<'tcx>(cx: &LateContext<'tcx>, ty: Ty<'tcx>, span: rustc_span::Span) {
    let ty = ty.peel_refs();
    if let ty::Adt(adt_def, _) = ty.kind() {
        if is_nondet_type(cx, adt_def.did()) {
            cx.struct_span_lint(&RS_NO_NONDET, span, |diag| {
                diag.build("non-deterministic collections forbidden in rs edition")
                    .code_str("RS507")
                    .help("use BTreeSet for deterministic iteration order")
                    .emit();
            });
        }
    }
}

/// Register the no-nondet lint pass in the lint store.
pub fn register_lints(store: &mut rustc_lint::LintStore) {
    store.register_lints(&[&RS_NO_NONDET]);
    store.register_late_pass(|_| Box::new(NoNondet));
}
