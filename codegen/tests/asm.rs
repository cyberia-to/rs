#![no_std]
#![no_main]

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    let input: i64 = 42;
    let result: i64;
    unsafe {
        core::arch::asm!("mov {0}, {1}", out(reg) result, in(reg) input);
    }
    result as i32
}

#[panic_handler]
fn ph(_: &core::panic::PanicInfo) -> ! { loop {} }
