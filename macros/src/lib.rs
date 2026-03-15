//! Rs proc-macros: addressed, step, deterministic, register, cell, bounded_async.
//!
//! Generates code targeting `rs_lang::` paths. Works with standard rustc
//! (proc-macro enforcement) and rsc (additional MIR-level checks).

extern crate proc_macro;

mod addressed;
mod bounded_async;
mod cell;
mod deterministic;
mod registers;
mod step;

use proc_macro::TokenStream;

/// Derive canonical serialization and particle identity for a struct or enum.
///
/// Generates `impl CanonicalSerialize` and `fn particle(&self) -> Particle`.
/// Rejects f32/f64 (RS302), raw pointers (RS303), HashMap (RS304),
/// usize/isize (RS305), and wide enum reprs (RS306).
#[proc_macro_derive(Addressed, attributes(addressed))]
pub fn derive_addressed(input: TokenStream) -> TokenStream {
    addressed::derive(input.into())
        .unwrap_or_else(|e| e.into_compile_error())
        .into()
}

/// Attribute macro that generates `StepReset` impl for a struct or a
/// reset function for a static item.
///
/// On structs: generates `impl StepReset` that resets all fields to
/// their zero/empty values at step boundaries.
///
/// On statics: emits the static unchanged and generates a hidden
/// `unsafe fn __rs_step_reset_<name>()` that resets the value.
/// Atomics use `store(0, SeqCst)`; other types re-assign the
/// original initializer.
#[proc_macro_attribute]
pub fn step(attr: TokenStream, item: TokenStream) -> TokenStream {
    step::expand(attr.into(), item.into())
        .unwrap_or_else(|e| e.into_compile_error())
        .into()
}

/// Attribute macro that checks a function body for non-deterministic constructs.
///
/// Token-level checks: f32/f64 (RS201/RS202), HashMap/HashSet (RS207),
/// rand:: (RS205), std::time::Instant (RS204), asm!/global_asm! (RS208).
#[proc_macro_attribute]
pub fn deterministic(attr: TokenStream, item: TokenStream) -> TokenStream {
    deterministic::expand(attr.into(), item.into())
        .unwrap_or_else(|e| e.into_compile_error())
        .into()
}

/// Attribute macro that wraps an async fn body with a deadline.
///
/// Usage: `#[bounded_async(Duration::from_millis(100))]`
#[proc_macro_attribute]
pub fn bounded_async(attr: TokenStream, item: TokenStream) -> TokenStream {
    bounded_async::expand(attr.into(), item.into())
        .unwrap_or_else(|e| e.into_compile_error())
        .into()
}

/// Attribute macro on modules that generates typed MMIO register access code.
///
/// Validates bit layouts, generates safe read/write/modify methods backed
/// by read_volatile/write_volatile. Errors: RS001-RS008.
#[proc_macro_attribute]
pub fn register(attr: TokenStream, item: TokenStream) -> TokenStream {
    registers::expand(attr.into(), item.into())
        .unwrap_or_else(|e| e.into_compile_error())
        .into()
}

/// Declarative macro for cell definitions.
///
/// Parses name, version, budget, heartbeat, state, step_state, methods,
/// channels, and migration blocks. Generates state structs, Cell trait impl,
/// error enum, CellMetadata, and public interface.
#[proc_macro]
pub fn cell(input: TokenStream) -> TokenStream {
    cell::expand(input.into())
        .unwrap_or_else(|e| e.into_compile_error())
        .into()
}
