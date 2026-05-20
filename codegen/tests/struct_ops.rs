// Phase 3 test: multi-function call with integer arguments.
// Expected exit code: 42  (10 + 32)
#![no_std]
#![no_main]

fn add_two(x: i32, y: i32) -> i32 {
    x + y
}

fn double(x: i32) -> i32 {
    add_two(x, x)
}

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    let a = add_two(10, 32);  // 42
    let b = double(0);        // 0
    add_two(a, b)             // 42
}

#[panic_handler]
fn ph(_: &core::panic::PanicInfo) -> ! { loop {} }
