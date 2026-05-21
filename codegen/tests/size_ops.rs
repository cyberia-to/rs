// Integration test: arithmetic with CheckedMul/CheckedAdd paths.
// 4 * 10 + 2 == 42.
#![no_std]
#![no_main]

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    let a: i32 = 4;
    a * 10 + 2
}

#[panic_handler]
fn ph(_: &core::panic::PanicInfo) -> ! { loop {} }
