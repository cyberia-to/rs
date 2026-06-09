#![no_std]
#![no_main]

#[no_mangle]
pub extern "C" fn multi_add(a: i32, b: i32) -> i32 { a + b }

#[no_mangle]
pub extern "C" fn multi_mul(a: i32, b: i32) -> i32 { a * b }

#[panic_handler]
fn ph(_: &core::panic::PanicInfo) -> ! { loop {} }
