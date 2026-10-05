//! Negative fixture: `let _ = <future>` (rejected by
//! `let_underscore_future = "deny"`).

/// Violating spawn: drops the future instead of awaiting or spawning it.
pub fn spawnish() {
    let _ = async { 1u32 };
}
