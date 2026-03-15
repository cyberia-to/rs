//! Rs lint passes — all registered via the rustc driver callbacks.

pub mod rs_addressed;
pub mod rs_bounded_async;
pub mod rs_deterministic;
pub mod rs_diag;
pub mod rs_no_dyn;
pub mod rs_no_heap;
pub mod rs_no_nondet;
pub mod rs_no_panic;
pub mod rs_step;

/// Register Rs lint passes in the lint store.
///
/// Attribute-triggered lints (deterministic, bounded_async, step, addressed)
/// are always registered — they only fire on annotated code.
///
/// Edition restriction lints (no_heap, no_dyn, no_panic, no_nondet) are
/// only registered when `--rs-edition` is passed.
pub fn register_all(store: &mut rustc_lint::LintStore, rs_edition: bool) {
    // Always active: triggered by user attributes
    rs_deterministic::register_lints(store);
    rs_bounded_async::register_lints(store);
    rs_step::register_lints(store);
    rs_addressed::register_lints(store);

    // Edition-gated: only with --rs-edition
    if rs_edition {
        rs_no_heap::register_lints(store);
        rs_no_dyn::register_lints(store);
        rs_no_panic::register_lints(store);
        rs_no_nondet::register_lints(store);
    }
}
