//! Negative fixture: `let _ = <must_use>` (rejected by
//! `let_underscore_must_use = "deny"`).

/// Pure answer; callers must use the return value.
#[must_use]
pub fn answer() -> u32 {
    42
}

/// Violating caller: silences the must-use result with `let _`.
pub fn ignore() {
    let _ = answer();
}
