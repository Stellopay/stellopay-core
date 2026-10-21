// `mock_contract.rs` is a native-only test fixture whose legacy traps are
// intentionally retained, so the guard must skip it by name.
pub fn legacy_failure() {
    panic!("intentionally untyped fixture trap");
}
