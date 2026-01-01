//! Negative fixture: `.unwrap()` (rejected by `unwrap_used = "deny"`).

/// Parse and double; `None` on unparseable input.
pub fn doubled(text: &str) -> Option<u32> {
    text.parse::<u32>().ok().map(|value| value * 2)
}

/// Violating caller: unwraps instead of handling `None`.
pub fn doubled_or_zero(text: &str) -> u32 {
    doubled(text).unwrap()
}
