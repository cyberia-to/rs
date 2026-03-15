// Test RS503: String forbidden

#[allow(rs_no_panic_unwind)]
fn main() {
    let _s: String = "hello".to_string(); //~ ERROR heap-allocated strings forbidden
}
