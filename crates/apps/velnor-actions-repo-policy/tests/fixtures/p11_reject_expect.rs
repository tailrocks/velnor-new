//! Negative fixture: `.expect()` (rejected by `expect_used = "deny"`).

/// Violating caller: expects instead of handling `None`.
pub fn doubled_or_zero(text: &str) -> u32 {
    text.parse::<u32>().ok().expect("num")
}
