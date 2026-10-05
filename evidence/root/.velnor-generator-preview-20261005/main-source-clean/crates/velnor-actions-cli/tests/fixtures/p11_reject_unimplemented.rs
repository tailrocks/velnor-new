//! Negative fixture: `unimplemented!` (rejected by `unimplemented = "deny"`).

/// Violating stub: unimplemented instead of returning a `Result`.
pub fn later() -> u32 {
    unimplemented!()
}
