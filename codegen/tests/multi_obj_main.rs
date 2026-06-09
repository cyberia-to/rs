#![no_std]
#![no_main]

extern "C" {
    fn multi_add(a: i32, b: i32) -> i32;
    fn multi_mul(a: i32, b: i32) -> i32;
    fn exit(code: i32) -> !;
}

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    let r = unsafe { multi_mul(multi_add(10, 32), 1) };
    unsafe { exit(r) }
}

#[panic_handler]
fn ph(_: &core::panic::PanicInfo) -> ! { loop {} }
