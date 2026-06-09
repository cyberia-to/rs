#![no_std]
#![no_main]

trait Compute {
    fn compute(&self, x: i32) -> i32;
}

struct Adder(i32);
struct Multiplier(i32);

impl Compute for Adder {
    fn compute(&self, x: i32) -> i32 { x + self.0 }
}

impl Compute for Multiplier {
    fn compute(&self, x: i32) -> i32 { x * self.0 }
}

fn run(c: &dyn Compute, x: i32) -> i32 {
    c.compute(x)
}

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    let a = Adder(12);
    let m = Multiplier(2);
    // run(&a, 9) = 21, run(&m, 21) = 42
    run(&m, run(&a, 9))
}

#[panic_handler]
fn ph(_: &core::panic::PanicInfo) -> ! { loop {} }
