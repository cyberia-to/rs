#![feature(rustc_private)]
#![feature(box_patterns)]
#![allow(unused_imports)]

// rustc_driver provides compiled code for all rustc_* crates as a dylib.
// Without this, the linker can't find the rlib form of those crates.
extern crate rustc_abi;
extern crate rustc_driver;
extern crate rustc_ast;
extern crate rustc_codegen_ssa;
extern crate rustc_data_structures;
extern crate rustc_errors;
extern crate rustc_hir;
extern crate rustc_metadata;
extern crate rustc_middle;
extern crate rustc_session;
extern crate rustc_span;
extern crate rustc_target;

mod backend;
mod codegen;
mod encoders;
mod lir;
mod arm64;
mod mir2lir;
mod link;

use rustc_codegen_ssa::traits::CodegenBackend;

#[no_mangle]
pub fn __rustc_codegen_backend() -> Box<dyn CodegenBackend> {
    Box::new(backend::TridentBackend)
}
