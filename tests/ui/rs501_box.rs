// Test RS501: Box forbidden
// expect-error: RS501

#[allow(rs_no_panic_unwind)]
fn main() {
    let _b: Box<i32> = Box::new(42); //~ ERROR heap allocation forbidden
}
