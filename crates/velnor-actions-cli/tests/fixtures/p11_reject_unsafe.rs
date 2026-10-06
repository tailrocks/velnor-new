//! Negative fixture: `unsafe` block (rejected by `unsafe_code = "forbid"`).

/// Read through a raw pointer; the `unsafe` block is the violation.
pub fn read_or_zero(slot: *const u8) -> u8 {
    unsafe { *slot }
}
