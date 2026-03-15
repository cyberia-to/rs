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

/// Register all Rs lint passes in the lint store.
pub fn register_all(store: &mut rustc_lint::LintStore) {
    rs_no_heap::register_lints(store);
    rs_no_dyn::register_lints(store);
    rs_no_panic::register_lints(store);
    rs_no_nondet::register_lints(store);
    rs_deterministic::register_lints(store);
    rs_bounded_async::register_lints(store);
    rs_step::register_lints(store);
    rs_addressed::register_lints(store);
}
