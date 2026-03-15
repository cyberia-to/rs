// Test RS504: dyn Trait forbidden

trait Foo { fn bar(&self); }

#[allow(rs_no_panic_unwind)]
fn takes_dyn(_x: &dyn Foo) {} //~ ERROR dynamic dispatch forbidden

fn main() {}
