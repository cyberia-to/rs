//! Edition restriction lint: RS506 — forbids unwinding panics.

use rustc_hir::def_id::CRATE_DEF_ID;
use rustc_lint::{LateContext, LateLintPass, LintContext};
use rustc_session::{declare_lint, declare_lint_pass};

declare_lint! { pub RS_NO_PANIC_UNWIND, Deny, "unwinding panic forbidden in rs edition (RS506)" }

declare_lint_pass!(NoPanicUnwind => [RS_NO_PANIC_UNWIND]);

impl<'tcx> LateLintPass<'tcx> for NoPanicUnwind {
    fn check_crate(&mut self, cx: &LateContext<'tcx>) {
        let is_abort = cx.tcx.sess.opts.cg.panic == Some(rustc_target::spec::PanicStrategy::Abort);
        if !is_abort {
            cx.span_lint(
                RS_NO_PANIC_UNWIND,
                cx.tcx.def_span(CRATE_DEF_ID),
                |diag| {
                    diag.primary_message("unwinding panic forbidden in rs edition (RS506)");
                    diag.help("set `panic = \"abort\"` in [profile.*] in Cargo.toml");
                },
            );
        }
    }
}

pub fn register_lints(store: &mut rustc_lint::LintStore) {
    store.register_lints(&[&RS_NO_PANIC_UNWIND]);
    store.register_late_pass(|_| Box::new(NoPanicUnwind));
}
