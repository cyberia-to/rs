//! Edition restriction lints: RS501, RS502, RS503, RS505.
//!
//! Forbids heap-allocating types in rs edition code. These four share the
//! same allow-attribute (`rs::heap`) because they all stem from the same
//! design constraint: Rs cells manage memory through bounded, compile-time-
//! sized containers instead of growable heap allocation.

use rustc_hir as hir;
use rustc_hir::def_id::DefId;
use rustc_lint::{LateContext, LateLintPass, LintContext};
use rustc_middle::ty::{self, Ty};
use rustc_session::{declare_lint, declare_lint_pass};
use rustc_span::sym;

use crate::rs_edition;

// ---------------------------------------------------------------------------
// Lint declarations
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Type path matching
// ---------------------------------------------------------------------------

/// Known heap-allocating type paths and their corresponding lint + error code.
const HEAP_TYPES: &[(&[&str], &'static rustc_lint::Lint, &str)] = &[
    // RS501: Box
    (&["alloc", "boxed", "Box"], &RS_NO_BOX, "RS501"),
    (&["std", "boxed", "Box"], &RS_NO_BOX, "RS501"),
    // RS502: Vec, VecDeque
    (&["alloc", "vec", "Vec"], &RS_NO_VEC, "RS502"),
    (&["std", "vec", "Vec"], &RS_NO_VEC, "RS502"),
    (&["alloc", "collections", "vec_deque", "VecDeque"], &RS_NO_VEC, "RS502"),
    (&["std", "collections", "VecDeque"], &RS_NO_VEC, "RS502"),
    // RS503: String
    (&["alloc", "string", "String"], &RS_NO_STRING, "RS503"),
    (&["std", "string", "String"], &RS_NO_STRING, "RS503"),
    // RS505: Arc, Rc
    (&["alloc", "sync", "Arc"], &RS_NO_REFCOUNT, "RS505"),
    (&["std", "sync", "Arc"], &RS_NO_REFCOUNT, "RS505"),
    (&["alloc", "rc", "Rc"], &RS_NO_REFCOUNT, "RS505"),
    (&["std", "rc", "Rc"], &RS_NO_REFCOUNT, "RS505"),
];

/// Help text keyed by error code.
fn help_for(code: &str) -> &'static str {
    match code {
        "RS501" => "use a stack value or Arena<T, N>",
        "RS502" => "use BoundedVec<T, N> with compile-time capacity",
        "RS503" => "use &str or ArrayString<N>",
        "RS505" => "use cell-owned state or bounded channels",
        _ => "",
    }
}

/// Check whether a resolved type's DefId matches any forbidden heap type.
fn check_adt_def_id(cx: &LateContext<'_>, def_id: DefId) -> Option<(&'static rustc_lint::Lint, &'static str)> {
    let def_path = cx.tcx.def_path_str(def_id);

    for &(segments, lint, code) in HEAP_TYPES {
        let expected = segments.join("::");
        if def_path == expected {
            return Some((lint, code));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// HIR walk
// ---------------------------------------------------------------------------

impl<'tcx> LateLintPass<'tcx> for NoHeap {
    /// Check type annotations in variable bindings, function parameters,
    /// return types, struct fields, and static/const declarations.
    fn check_ty(&mut self, cx: &LateContext<'tcx>, hir_ty: &'tcx hir::Ty<'tcx>) {
        rs_edition_guard!(cx);

        let ty = cx.typeck_results().node_type_opt(hir_ty.hir_id);
        let ty = match ty {
            Some(t) => t,
            None => return,
        };

        check_type_for_heap(cx, ty, hir_ty.span);
    }

    /// Check expressions that construct heap types (e.g. `Box::new(x)`,
    /// `Vec::new()`, `String::from("x")`).
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx hir::Expr<'tcx>) {
        rs_edition_guard!(cx);

        let ty = cx.typeck_results().expr_ty_adjusted(expr);
        check_type_for_heap(cx, ty, expr.span);
    }
}

/// Inspect a resolved type and emit the appropriate diagnostic if it is
/// a forbidden heap type.
fn check_type_for_heap<'tcx>(cx: &LateContext<'tcx>, ty: Ty<'tcx>, span: rustc_span::Span) {
    // Peel through references and smart pointers to find the ADT.
    let ty = ty.peel_refs();

    if let ty::Adt(adt_def, _) = ty.kind() {
        if let Some((lint, code)) = check_adt_def_id(cx, adt_def.did()) {
            cx.struct_span_lint(lint, span, |diag| {
                diag.build(lint.desc)
                    .code_str(code)
                    .help(help_for(code))
                    .emit();
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

/// Register the no-heap lint pass in the lint store.
pub fn register_lints(store: &mut rustc_lint::LintStore) {
    store.register_lints(&[&RS_NO_BOX, &RS_NO_VEC, &RS_NO_STRING, &RS_NO_REFCOUNT]);
    store.register_late_pass(|_| Box::new(NoHeap));
}

#[cfg(test)]
mod tests {
    #[test]
    fn help_text_covers_all_codes() {
        for code in &["RS501", "RS502", "RS503", "RS505"] {
            assert!(!super::help_for(code).is_empty(), "missing help for {}", code);
        }
    }
}
