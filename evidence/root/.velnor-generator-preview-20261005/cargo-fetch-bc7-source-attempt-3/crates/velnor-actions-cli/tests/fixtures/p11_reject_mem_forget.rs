//! Negative fixture: `mem::forget` (rejected by `mem_forget = "deny"`).

/// Violating leak: forgets instead of dropping or returning ownership.
pub fn leak(text: String) {
    std::mem::forget(text);
}
