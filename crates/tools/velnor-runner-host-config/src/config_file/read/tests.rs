use std::ffi::OsStr;
use std::fs::{self, File};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use rustix::fs::{Mode, OFlags, open};

use super::{MAX_HOST_CONFIG_BYTES, read_leaf};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, String> {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let id = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-config-snapshot-{}-{id}",
            std::process::id()
        ));
        fs::create_dir(&path).map_err(|error| error.to_string())?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
        Ok(Self(path))
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.0);
    }
}

fn open_temp_dir(path: &std::path::Path) -> Result<File, String> {
    open(
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map(File::from)
    .map_err(|error| error.to_string())
}

#[test]
fn bounded_reader_reads_one_regular_file_and_rejects_oversize() -> Result<(), String> {
    let directory = TempDir::new()?;
    let file_path = directory.path().join("host.toml");
    fs::write(&file_path, b"schema = 1\n").map_err(|error| error.to_string())?;
    fs::set_permissions(&file_path, fs::Permissions::from_mode(0o600))
        .map_err(|error| error.to_string())?;
    let directory_fd = open_temp_dir(directory.path())?;
    let read = read_leaf(&directory_fd, OsStr::new("host.toml"), |_| Ok(true))
        .map_err(|_| "regular config read failed".to_owned())?;
    if read.as_deref() != Some(&b"schema = 1\n"[..]) {
        return Err("bounded reader returned different bytes".to_owned());
    }

    fs::write(&file_path, vec![b'x'; MAX_HOST_CONFIG_BYTES + 1])
        .map_err(|error| error.to_string())?;
    if read_leaf(&directory_fd, OsStr::new("host.toml"), |_| Ok(true)).is_ok() {
        return Err("oversized config was accepted".to_owned());
    }
    Ok(())
}

#[test]
fn bounded_reader_rejects_symlinks_and_fifo_without_waiting_for_a_writer() -> Result<(), String> {
    let directory = TempDir::new()?;
    let directory_fd = open_temp_dir(directory.path())?;
    let target = directory.path().join("target.toml");
    fs::write(&target, b"schema = 1\n").map_err(|error| error.to_string())?;
    fs::set_permissions(&target, fs::Permissions::from_mode(0o600))
        .map_err(|error| error.to_string())?;
    symlink(&target, directory.path().join("link.toml")).map_err(|error| error.to_string())?;
    if read_leaf(&directory_fd, OsStr::new("link.toml"), |_| Ok(true)).is_ok() {
        return Err("symlink config was accepted".to_owned());
    }

    super::super::test_fifo::create_fifo(directory.path(), "waiting.toml")?;
    if read_leaf(&directory_fd, OsStr::new("waiting.toml"), |_| Ok(true)).is_ok() {
        return Err("FIFO config was accepted".to_owned());
    }
    Ok(())
}
