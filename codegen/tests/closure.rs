#![no_std]
#![no_main]

fn apply<F: FnOnce() -> i32>(f: F) -> i32 { f() }

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    let a = 10i32;
    let b = 32i32;
    apply(move || a + b)
}

#[panic_handler]
fn ph(_: &core::panic::PanicInfo) -> ! { loop {} }
