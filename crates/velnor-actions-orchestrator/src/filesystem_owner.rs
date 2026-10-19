//! Read-only filesystem ownership identity shared by private staging and artifacts.

/// Read the effective user ID for file ownership checks.
#[cfg(unix)]
#[must_use]
pub fn effective_uid() -> u32 {
    rustix::process::geteuid().as_raw()
}
