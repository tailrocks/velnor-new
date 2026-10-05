//! Negative fixture: undocumented `unsafe` block (rejected by
//! `undocumented_unsafe_blocks = "deny"`).

/// Kernel behind the boundary; the missing `SAFETY` comment is the violation.
unsafe fn kernel() -> u32 {
    7
}

/// Violating caller: `unsafe` block without a `SAFETY` comment.
pub fn seven() -> u32 {
    unsafe { kernel() }
}
