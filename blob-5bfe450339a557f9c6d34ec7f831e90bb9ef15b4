//! Negative fixture: unfulfilled `#[expect]` (rejected by
//! `unfulfilled_lint_expectations = "deny"`).

/// Violating expectation: `dead_code` never fires because `caller` uses this.
#[expect(dead_code, reason = "fixture proves unfulfilled fails")]
pub fn used() -> u32 {
    3
}

/// Caller that fulfills nothing: the expectation above stays unfulfilled.
pub fn caller() -> u32 {
    used()
}
