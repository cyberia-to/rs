//! Edition restriction lint: RS506.
//!
//! Forbids unwinding panics in rs edition. Only `panic = "abort"` is
//! permitted — stack unwinding adds complexity (landing pads, drop
//! handlers during unwind) that an OS kernel does not need. No opt-out.

use rustc_lint::{LateContext, LateLintPass, LintContext};
use rustc_session::{declare_lint, declare_lint_pass};

use crate::rs_edition;

declare_lint! {
    /// Unwinding panic forbidden in rs edition. No opt-out.
    pub RS_NO_PANIC_UNWIND,
    Deny,
    "unwinding panic forbidden in rs edition (RS506)"
}

declare_lint_pass!(NoPanicUnwind => [RS_NO_PANIC_UNWIND]);

impl<'tcx> LateLintPass<'tcx> for NoPanicUnwind {
    /// Check the crate-level panic strategy once at the start of linting.
    /// If the strategy is not abort, emit an error on the crate root span.
    fn check_crate(&mut self, cx: &LateContext<'tcx>) {
        rs_edition_guard!(cx);

        let panic_strategy = cx.tcx.sess.panic_strategy();
        if panic_strategy != rustc_target::spec::PanicStrategy::Abort {
            cx.struct_span_lint(
                &RS_NO_PANIC_UNWIND,
                cx.tcx.def_span(rustc_hir::CRATE_DEF_ID),
                |diag| {
                    diag.build("unwinding panic forbidden in rs edition")
                        .code_str("RS506")
                        .help("use Result for recoverable errors, or abort for unrecoverable")
                        .note("set `panic = \"abort\"` in [profile.*] in Cargo.toml")
                        .emit();
                },
            );
        }
    }
}

/// Register the no-panic-unwind lint pass in the lint store.
pub fn register_lints(store: &mut rustc_lint::LintStore) {
    store.register_lints(&[&RS_NO_PANIC_UNWIND]);
    store.register_late_pass(|_| Box::new(NoPanicUnwind));
}
