//! Negative fixture: `panic!` (rejected by `panic = "deny"`).

/// Violating branch: panics instead of returning a `Result`.
pub fn boom(flag: bool) -> u32 {
    if flag { panic!("boom") } else { 0 }
}
