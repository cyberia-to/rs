// Test RS505: Arc/Rc forbidden

use std::sync::Arc;

#[allow(rs_no_panic_unwind)]
fn main() {
    let _a: Arc<i32> = Arc::new(42); //~ ERROR reference counting forbidden
}
