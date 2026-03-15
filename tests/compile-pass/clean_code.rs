// Clean Rs code — no heap, no dyn, no floats, deterministic.
// Should compile without errors when using -C panic=abort.

#[allow(rs_no_panic_unwind)]
fn checked_add(a: u32, b: u32) -> u32 {
    a.checked_add(b).unwrap_or(u32::MAX)
}

fn main() {
    let x = checked_add(1, 2);
    assert_eq!(x, 3);
}
