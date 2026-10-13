//! Negative fixture: `todo!` (rejected by `todo = "deny"`).

/// Violating stub: unfinished instead of returning a `Result`.
pub fn later() -> u32 {
    todo!()
}
