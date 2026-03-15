// Test RS506: unwinding panic forbidden
// This file is compiled WITHOUT -C panic=abort, so the lint should fire.

fn main() {
    println!("this should trigger RS506"); //~ ERROR unwinding panic forbidden
}
