//! Negative fixture: ignored `#[must_use]` result (rejected by
//! `unused_must_use = "deny"`).

/// Pure answer; callers must use the return value.
#[must_use]
pub fn answer() -> u32 {
    42
}

/// Violating caller: drops the must-use result as a statement.
pub fn ignore() {
    answer();
}
