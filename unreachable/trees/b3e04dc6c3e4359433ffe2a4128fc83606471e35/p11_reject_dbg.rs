//! Negative fixture: `dbg!` (rejected by `dbg_macro = "deny"`).

/// Violating trace: debug macro left in shipped code.
pub fn show(value: u32) -> u32 {
    dbg!(value)
}
