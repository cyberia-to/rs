#![no_std]
#![no_main]

extern "C" {
    fn write(fd: i32, buf: *const u8, len: usize) -> isize;
    fn exit(code: i32) -> !;
}

#[no_mangle]
static MSG: [u8; 14] = *b"Hello, world!\n";

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    unsafe {
        let ptr: *const u8 = &MSG as *const [u8; 14] as *const u8;
        write(1, ptr, 14);
        exit(0);
    }
}

#[panic_handler]
fn ph(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
