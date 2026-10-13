use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub(super) struct TemporaryTree(PathBuf);

impl TemporaryTree {
    pub(super) fn new(path: PathBuf) -> Self {
        Self(path)
    }
}

impl Drop for TemporaryTree {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            eprintln!("action_mbx_path_cleanup_failed:{:?}", error.kind());
        }
    }
}

pub(super) fn new_folder(parent: &Path, name: &str) -> io::Result<PathBuf> {
    let path = parent.join(name);
    std::fs::create_dir(&path)?;
    Ok(path)
}

pub(super) fn save_bytes(path: &Path, contents: &[u8]) -> io::Result<()> {
    std::fs::write(path, contents)
}

pub(super) fn make_executable(path: &Path) -> io::Result<()> {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
}
