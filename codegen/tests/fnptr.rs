// Phase 3 test: signed comparisons and branching.
// Expected exit code: 7
#![no_std]
#![no_main]

fn abs_diff(a: i32, b: i32) -> i32 {
    if a > b { a - b } else { b - a }
}

fn clamp(x: i32, lo: i32, hi: i32) -> i32 {
    if x < lo { lo } else if x > hi { hi } else { x }
}

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    let d = abs_diff(50, 43);  // 7
    clamp(d, 0, 42)            // 7 (already in range)
}

#[panic_handler]
fn ph(_: &core::panic::PanicInfo) -> ! { loop {} }
