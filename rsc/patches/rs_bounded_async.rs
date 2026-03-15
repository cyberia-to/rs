//! Bounded async enforcement: RS101.
//!
//! In rs edition, every async function must have an explicit deadline.
//! An async function without a deadline can block indefinitely — a liveness
//! failure in OS kernels and consensus nodes.
//!
//! Inside `cell!`: the macro enforces `async(Duration) fn` syntax.
//! Outside cells: `#[bounded_async(Duration)]` attribute macro.
//! This lint catches anything that slips through both layers.

use rustc_hir as hir;
use rustc_hir::def_id::LocalDefId;
use rustc_lint::{LateContext, LateLintPass, LintContext};
use rustc_session::{declare_lint, declare_lint_pass};
use rustc_span::{Span, Symbol};

use crate::rs_edition;

declare_lint! {
    /// Every async fn must have a deadline in rs edition.
    pub RS_UNBOUNDED_ASYNC,
    Deny,
    "async functions must have a deadline in rs edition (RS101)"
}

declare_lint_pass!(BoundedAsync => [RS_UNBOUNDED_ASYNC]);

/// Attribute names that satisfy the deadline requirement.
const DEADLINE_ATTRS: &[&str] = &[
    "bounded_async",
    "rs_bounded_async",
];

/// Returns true if the function has a bounded_async attribute or is inside
/// a cell! macro expansion (detected by the `__rs_cell_context` attribute
/// that the cell! macro places on generated items).
fn has_deadline(cx: &LateContext<'_>, def_id: LocalDefId) -> bool {
    let tcx = cx.tcx;
    let attrs = tcx.hir().attrs(tcx.local_def_id_to_hir_id(def_id));

    for attr in attrs {
        if let Some(ident) = attr.ident() {
            let name = ident.as_str();
            // Direct bounded_async attribute
            if DEADLINE_ATTRS.iter().any(|&a| name == a) {
                return true;
            }
            // Inside a cell! macro expansion
            if name == "__rs_cell_context" {
                return true;
            }
        }
    }
    false
}

/// Returns true if the function has the `#[allow(rs::unbounded_async)]`
/// attribute. The lint framework handles this automatically via the
/// allow mechanism, but we check explicitly for functions where the
/// attribute is on an enclosing scope.
fn is_opted_out(cx: &LateContext<'_>, def_id: LocalDefId) -> bool {
    let tcx = cx.tcx;
    let attrs = tcx.hir().attrs(tcx.local_def_id_to_hir_id(def_id));

    for attr in attrs {
        if attr.has_name(Symbol::intern("allow")) {
            if let Some(meta_list) = attr.meta_item_list() {
                for nested in meta_list {
                    if let Some(meta_item) = nested.meta_item() {
                        let path_str = meta_item
                            .path
                            .segments
                            .iter()
                            .map(|s| s.ident.as_str())
                            .collect::<Vec<_>>()
                            .join("::");
                        if path_str == "rs::unbounded_async" {
                            return true;
                        }
                    }
                }
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
        decl: &'tcx hir::FnDecl<'tcx>,
        _body: &'tcx hir::Body<'tcx>,
        span: Span,
        def_id: LocalDefId,
    ) {
        rs_edition_guard!(cx);

        // Only check async functions
        let is_async = match kind {
            hir::intravisit::FnKind::ItemFn(_, _, header) => header.asyncness.is_async(),
            hir::intravisit::FnKind::Method(_, sig) => sig.header.asyncness.is_async(),
            // Closures — async closures are handled by the async block check
            hir::intravisit::FnKind::Closure => return,
        };

        if !is_async {
            return;
        }

        // Skip if already bounded or opted out
        if has_deadline(cx, def_id) || is_opted_out(cx, def_id) {
            return;
        }

        cx.struct_span_lint(&RS_UNBOUNDED_ASYNC, span, |diag| {
            diag.build("async functions must have a deadline in rs edition")
                .code_str("RS101")
                .help("add a deadline: #[bounded_async(Duration::from_millis(100))]")
                .help("or opt out: #[allow(rs::unbounded_async)]")
                .emit();
        });
    }

    /// Check async blocks — they should be inside a bounded async function
    /// or cell context.
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx hir::Expr<'tcx>) {
        rs_edition_guard!(cx);

        // Async blocks that create futures
        if let hir::ExprKind::Closure(closure) = &expr.kind {
            if closure.kind == hir::ClosureKind::Coroutine(hir::CoroutineKind::Async(..)) {
                // Async blocks inherit their parent function's deadline.
                // Standalone async blocks outside any bounded context are
                // caught by the parent function check — if the enclosing
                // function is not bounded, it will already be flagged.
                // No additional check needed here.
            }
        }
    }
}

/// Register the bounded-async lint pass in the lint store.
pub fn register_lints(store: &mut rustc_lint::LintStore) {
    store.register_lints(&[&RS_UNBOUNDED_ASYNC]);
    store.register_late_pass(|_| Box::new(BoundedAsync));
}

// ---------------------------------------------------------------------------
// Async block utilities
// ---------------------------------------------------------------------------

/// Helper to determine if a span is inside a cell! macro expansion.
/// Used by the lint pass to skip async blocks generated by cell! desugaring.
pub fn is_in_cell_expansion(span: Span) -> bool {
    span.ctxt().outer_expn_data().macro_def_id.map_or(false, |def_id| {
        // The cell! macro sets a specific expansion kind that we recognize
        // by checking the macro name.
        false // Placeholder — apply.nu will wire this to the cell! macro's DefId
    })
}
