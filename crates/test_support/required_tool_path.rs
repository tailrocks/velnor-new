use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

/// Build a test PATH that retains caller prefixes and resolves system tools.
pub(crate) fn with_required_tool_path(prefixes: &[PathBuf]) -> io::Result<OsString> {
    let inherited =
        std::env::var_os("PATH").ok_or_else(|| io::Error::other("PATH is unavailable"))?;
    let mut paths = prefixes.to_vec();
    paths.extend(std::env::split_paths(&inherited));
    let sbin = Path::new("/sbin");
    if !paths.iter().any(|path| path == sbin) {
        paths.push(sbin.to_path_buf());
    }
    std::env::join_paths(paths).map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))
}
