//! Owner lookup for the current process.

/// Gives the effective user ID for filesystem owner checks.
#[must_use]
pub fn effective_uid() -> u32 {
    rustix::process::geteuid().as_raw()
}
