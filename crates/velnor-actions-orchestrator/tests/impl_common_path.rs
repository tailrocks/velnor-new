use std::ffi::OsString;
use std::path::PathBuf;

/// Add macOS's system `sha256sum` directory to the isolated test PATH.
pub(crate) fn test_path_with_sbin() -> Result<OsString, std::env::JoinPathsError> {
    let current = std::env::var_os("PATH").unwrap_or_default();
    let mut paths = std::env::split_paths(&current).collect::<Vec<_>>();
    let sbin = PathBuf::from("/sbin");
    if !paths.contains(&sbin) {
        paths.push(sbin);
    }
    std::env::join_paths(paths)
}
