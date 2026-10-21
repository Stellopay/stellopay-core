// Equality assertions are rejected just like the plain assertion macro.
pub fn guarded(left: i32, right: i32) {
    assert_eq!(left, right);
    assert_ne!(left, right);
}
