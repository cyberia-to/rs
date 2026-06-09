#![no_std]
#![no_main]

enum Shape {
    Circle(i32),
    Rect(i32, i32),
}

fn area(s: Shape) -> i32 {
    match s {
        Shape::Circle(r) => r * r,   // use r*r not pi*r*r (no floats)
        Shape::Rect(w, h) => w * h,
    }
}

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    // Circle(6) → 36, Rect(3,2) → 6, sum = 42
    area(Shape::Circle(6)) + area(Shape::Rect(3, 2))
}

#[panic_handler]
fn ph(_: &core::panic::PanicInfo) -> ! { loop {} }
