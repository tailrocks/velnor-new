//! Positive control: valid code with no allows; must pass every deny
//! flag applied to the negative fixtures.

/// Saturating add; total, pure, and lint-clean.
pub fn add(left: u32, right: u32) -> u32 {
    left.saturating_add(right)
}
