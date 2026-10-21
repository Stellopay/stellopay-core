// Deployable contract source that still uses the original opaque traps. The
// guard must reject `.unwrap()`, `.expect(`, and `panic!` as before.
pub fn guarded(value: Option<i128>) -> i128 {
    let inner = value.unwrap();
    let _ = value.expect("present");
    if inner < 0 {
        panic!("negative");
    }
    inner
}
