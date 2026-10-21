// Deployable contract source with only typed error handling. The trap guard
// must accept this file.
pub fn guarded(value: i128) -> Result<(), i128> {
    if value <= 0 {
        return Err(13);
    }
    Ok(())
}
