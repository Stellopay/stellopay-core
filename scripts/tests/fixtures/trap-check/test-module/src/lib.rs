// In-file `#[cfg(test)]` modules are part of the test surface and are exempt
// from the guard even though they use the assertion macros.
pub fn guarded(value: i128) -> i128 {
    value
}

#[cfg(test)]
mod test {
    use super::guarded;

    #[test]
    fn it_works() {
        assert_eq!(guarded(1), 1);
        assert!(true);
    }
}
