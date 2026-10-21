// Deployable contract source that traps with an untyped string message. The
// trap guard must reject this file.
pub fn guarded(amount: i128) {
    assert!(amount > 0, "Amount must be positive");
}
