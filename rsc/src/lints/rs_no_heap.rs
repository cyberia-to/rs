//! Edition restriction lints: RS501, RS502, RS503, RS505.
//!
//! Forbids heap-allocating types in rs edition code.

use rustc_hir as hir;
use rustc_lint::{LateContext, LateLintPass, LintContext};
use rustc_middle::ty;
use rustc_session::{declare_lint, declare_lint_pass};

declare_lint! {
    /// Heap allocation via `Box::new()` forbidden in rs edition.
    pub RS_NO_BOX,
    Deny,
    "heap allocation forbidden in rs edition (RS501)"
}

declare_lint! {
    /// Growable collections (`Vec`, `VecDeque`) forbidden in rs edition.
    pub RS_NO_VEC,
    Deny,
    "growable collections forbidden in rs edition (RS502)"
}

declare_lint! {
    /// Heap-allocated strings (`String`) forbidden in rs edition.
    pub RS_NO_STRING,
    Deny,
    "heap-allocated strings forbidden in rs edition (RS503)"
}

declare_lint! {
    /// Reference counting (`Arc`, `Rc`) forbidden in rs edition.
    pub RS_NO_REFCOUNT,
    Deny,
    "reference counting forbidden in rs edition (RS505)"
}

declare_lint_pass!(NoHeap => [RS_NO_BOX, RS_NO_VEC, RS_NO_STRING, RS_NO_REFCOUNT]);

impl<'tcx> LateLintPass<'tcx> for NoHeap {
    fn check_ty(&mut self, cx: &LateContext<'tcx>, hir_ty: &'tcx hir::Ty<'tcx, hir::AmbigArg>) {
        let Some(ty) = cx.maybe_typeck_results().and_then(|r| r.node_type_opt(hir_ty.hir_id)) else {
            return;
        };

        let ty = ty.peel_refs();
        if let ty::Adt(adt_def, _) = ty.kind() {
            let path = cx.tcx.def_path_str(adt_def.did());
            if let Some((lint, code, help)) = match_heap_type(&path) {
                cx.span_lint(lint, hir_ty.span, |diag| {
                    diag.primary_message(format!("{} ({})", lint.desc, code));
                    diag.help(help);
                });
            }
        }
    }
}

fn match_heap_type(path: &str) -> Option<(&'static rustc_lint::Lint, &'static str, &'static str)> {
    match path {
        "alloc::boxed::Box" | "std::boxed::Box" =>
            Some((&RS_NO_BOX, "RS501", "use a stack value or Arena<T, N>")),
        "alloc::vec::Vec" | "std::vec::Vec" =>
            Some((&RS_NO_VEC, "RS502", "use BoundedVec<T, N> with compile-time capacity")),
        "alloc::string::String" | "std::string::String" =>
            Some((&RS_NO_STRING, "RS503", "use &str or ArrayString<N>")),
        "alloc::sync::Arc" | "std::sync::Arc" =>
            Some((&RS_NO_REFCOUNT, "RS505", "use cell-owned state or bounded channels")),
        "alloc::rc::Rc" | "std::rc::Rc" =>
            Some((&RS_NO_REFCOUNT, "RS505", "use cell-owned state or bounded channels")),
        _ => None,
    }
}

pub fn register_lints(store: &mut rustc_lint::LintStore) {
    store.register_lints(&[&RS_NO_BOX, &RS_NO_VEC, &RS_NO_STRING, &RS_NO_REFCOUNT]);
    store.register_late_pass(|_| Box::new(NoHeap));
}
