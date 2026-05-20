// Integration test source: compiled by rustc with -Z codegen-backend=libcodegen.dylib
// Expected: exits with code 42. No stdlib, no LLVM, no ld64.
#![no_std]
#![no_main]

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    42
}

#[panic_handler]
fn ph(_: &core::panic::PanicInfo) -> ! { loop {} }
