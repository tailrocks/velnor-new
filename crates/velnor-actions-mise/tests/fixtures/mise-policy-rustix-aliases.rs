use rustix as process_sys;
use rustix::fs as stage_fs;
use rustix::fs::open as open_stage;
use rustix::fs::{Mode, OFlags};
use std::fs::File;
use std::io;
use std::path::Path;

fn flags() -> (OFlags, Mode) {
    (
        OFlags::WRONLY | OFlags::CREATE | OFlags::TRUNC,
        Mode::RUSR | Mode::WUSR,
    )
}

pub fn imported_open(path: &Path) -> io::Result<File> {
    let (flags, mode) = flags();
    open_stage(path, flags, mode)
        .map(File::from)
        .map_err(io::Error::from)
}

pub fn module_open(path: &Path) -> io::Result<File> {
    let (flags, mode) = flags();
    stage_fs::open(path, flags, mode)
        .map(File::from)
        .map_err(io::Error::from)
}

pub fn crate_open(path: &Path) -> io::Result<File> {
    let (flags, mode) = flags();
    process_sys::fs::open(path, flags, mode)
        .map(File::from)
        .map_err(io::Error::from)
}
