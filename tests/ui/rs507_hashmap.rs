// Test RS507: HashMap forbidden

use std::collections::HashMap;

#[allow(rs_no_panic_unwind)]
fn main() {
    let _m: HashMap<String, i32> = HashMap::new(); //~ ERROR non-deterministic collections forbidden
}
