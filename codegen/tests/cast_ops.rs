// Phase 3 test: integer casts (ZeroExt / SignExt).
// Expected exit code: 42
#![no_std]
#![no_main]

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    let a: i8  = -20i8;
    let b: i32 = a as i32;   // sign-extend: -20
    let c: u8  = 62u8;
    let d: i32 = c as i32;   // zero-extend: 62
    (b + d) as i32            // -20 + 62 = 42
}

#[panic_handler]
fn ph(_: &core::panic::PanicInfo) -> ! { loop {} }
