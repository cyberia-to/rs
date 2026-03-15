// BTreeMap is allowed — deterministic iteration order.

use std::collections::BTreeMap;

#[allow(rs_no_panic_unwind)]
fn main() {
    let mut m = BTreeMap::new();
    m.insert("a", 1);
    m.insert("b", 2);
    for (k, v) in &m {
        println!("{}: {}", k, v);
    }
}
