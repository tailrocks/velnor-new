#[cfg(unix)]
pub(crate) fn effective_uid() -> u32 {
    rustix::process::geteuid().as_raw()
}

#[path = "held_directory.rs"]
pub(crate) mod held_directory;
#[path = "held_file.rs"]
pub(crate) mod held_file;
#[path = "owned_tempdir.rs"]
pub(crate) mod owned_tempdir;
