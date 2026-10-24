use std::path::{Path, PathBuf};

use super::{create_new_regular, read_bounded_regular};
use crate::error::HostError;

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> Result<Self, HostError> {
        let path = std::env::temp_dir().join(format!(
            "velnor-resource-probe-files-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&path).map_err(|_| HostError::Path)?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("temporary fixture cleanup succeeds");
    }
}

#[test]
fn reads_only_the_held_bounded_regular_file() -> Result<(), HostError> {
    use std::os::unix::fs::symlink;

    let directory = TempDirectory::new()?;
    let target = directory.path().join("target");
    std::fs::write(&target, b"safe bytes").map_err(|_| HostError::Path)?;
    assert_eq!(read_bounded_regular(&target, 10)?, b"safe bytes");
    assert_eq!(read_bounded_regular(&target, 4), Err(HostError::Frame));

    let link = directory.path().join("link");
    symlink(&target, &link).map_err(|_| HostError::Path)?;
    assert!(read_bounded_regular(&link, 10).is_err());
    assert_eq!(
        std::fs::read(&target).map_err(|_| HostError::Path)?,
        b"safe bytes"
    );
    Ok(())
}

#[test]
fn rejects_a_fifo_without_blocking() -> Result<(), HostError> {
    let directory = TempDirectory::new()?;
    let fifo = directory.path().join("input");
    let status = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .map_err(|_| HostError::Path)?;
    if !status.success() {
        return Err(HostError::Path);
    }
    assert_eq!(read_bounded_regular(&fifo, 32), Err(HostError::Frame));
    Ok(())
}

#[test]
fn creates_exclusively_and_preserves_symlink_targets() -> Result<(), HostError> {
    use std::os::unix::fs::symlink;

    let directory = TempDirectory::new()?;
    let output = directory.path().join("manifest.json");
    create_new_regular(&output, b"new manifest\n")?;
    assert_eq!(
        std::fs::read(&output).map_err(|_| HostError::Path)?,
        b"new manifest\n"
    );
    assert!(create_new_regular(&output, b"replacement\n").is_err());
    assert_eq!(
        std::fs::read(&output).map_err(|_| HostError::Path)?,
        b"new manifest\n"
    );

    let target = directory.path().join("target");
    std::fs::write(&target, b"preserve me").map_err(|_| HostError::Path)?;
    let link = directory.path().join("linked-output");
    symlink(&target, &link).map_err(|_| HostError::Path)?;
    assert!(create_new_regular(&link, b"overwrite me").is_err());
    assert!(
        std::fs::symlink_metadata(&link)
            .map_err(|_| HostError::Path)?
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        std::fs::read(&target).map_err(|_| HostError::Path)?,
        b"preserve me"
    );
    Ok(())
}

#[test]
fn rejects_a_symlink_as_the_held_parent() -> Result<(), HostError> {
    use std::os::unix::fs::symlink;

    let directory = TempDirectory::new()?;
    let real_parent = directory.path().join("real");
    std::fs::create_dir(&real_parent).map_err(|_| HostError::Path)?;
    let linked_parent = directory.path().join("linked");
    symlink(&real_parent, &linked_parent).map_err(|_| HostError::Path)?;
    assert!(create_new_regular(&linked_parent.join("output"), b"data").is_err());
    assert!(!real_parent.join("output").exists());
    Ok(())
}
