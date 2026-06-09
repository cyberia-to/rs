#![no_std]
#![no_main]

fn apply_twice<F: Fn(i32) -> i32>(f: F, x: i32) -> i32 {
    f(f(x))
}

fn apply_mut<F: FnMut(i32) -> i32>(mut f: F, x: i32) -> i32 {
    f(x)
}

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    // apply_twice(|x| x + 10, 1) = 21
    // apply_mut(|x| x * 2, 21) = 42
    let a = apply_twice(|x| x + 10, 1);
    apply_mut(|x| x * 2, a)
}

#[panic_handler]
fn ph(_: &core::panic::PanicInfo) -> ! { loop {} }
