// Test that #[allow(...)] attributes silence Rs lints.

#[allow(rs_no_vec)]
fn uses_vec() -> Vec<i32> {
    vec![1, 2, 3]
}

#[allow(rs_no_string)]
fn uses_string() -> String {
    "hello".to_string()
}

#[allow(rs_no_dyn)]
fn uses_dyn(x: &dyn std::fmt::Debug) {
    let _ = x;
}

#[allow(rs_no_panic_unwind)]
fn main() {
    let v = uses_vec();
    let s = uses_string();
    uses_dyn(&42);
    println!("{:?} {}", v, s);
}
