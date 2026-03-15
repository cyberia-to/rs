// Test RS502: Vec forbidden

#[allow(rs_no_panic_unwind)]
fn main() {
    let _v: Vec<i32> = vec![1, 2, 3]; //~ ERROR growable collections forbidden
}
