//! Read-only filesystem ownership identity shared by private staging and artifacts.

/// Read an owner id without trusting user-controlled environment variables.
/// Every ownership check reads the live effective UID; no value is cached.
#[cfg(unix)]
pub(crate) fn effective_uid() -> u32 {
    rustix::process::geteuid().as_raw()
}

#[path = "held_directory.rs"]
pub(crate) mod held_directory;
#[path = "held_file.rs"]
pub(crate) mod held_file;
