// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{OrchestratorError, internal::internal};
use std::path::Path;

#[cfg(unix)]
pub(crate) fn read_inventory(
    root: &InventoryRoot,
    path: &str,
    mode: &str,
    object: &str,
) -> Result<(u64, String, Option<String>), OrchestratorError> {
    let parent = open_parent(root, path)?;
    let name = path.rsplit('/').next().ok_or_else(invalid)?;
    if mode == "120000" {
        let mut buffer = [0u8; 4097];
        let len =
            rustix::fs::readlinkat_raw(&parent, name, &mut buffer[..]).map_err(|_| invalid())?;
        let bytes = &buffer[..len];
        if bytes.is_empty()
            || bytes.len() > 4096
            || crate::release_policy_admission::git_blob_sha1(bytes)? != object
        {
            return Err(invalid());
        }
        let target = std::str::from_utf8(bytes)
            .map_err(|_| invalid())?
            .to_owned();
        return Ok((
            u64::try_from(bytes.len()).map_err(|_| invalid())?,
            crate::generator::sha256_hex(bytes),
            Some(target),
        ));
    }
    let (size, digest) = read_regular(&parent, name, mode, object)?;
    Ok((size, digest, None))
}

#[cfg(not(unix))]
pub(crate) fn read_inventory(
    _root: &InventoryRoot,
    _path: &str,
    _mode: &str,
    _object: &str,
) -> Result<(u64, String, Option<String>), OrchestratorError> {
    Err(invalid())
}

fn invalid() -> OrchestratorError {
    internal("crate_release_inventory_invalid")
}

#[cfg(unix)]
fn open_parent(root: &InventoryRoot, path: &str) -> Result<rustix::fd::OwnedFd, OrchestratorError> {
    use rustix::fs::{Mode, OFlags, openat};
    let mut parent = rustix::io::dup(&root.fd).map_err(|_| invalid())?;
    let mut components = path.split('/').peekable();
    while let Some(component) = components.next() {
        if component.is_empty() || component == "." || component == ".." {
            return Err(invalid());
        }
        if components.peek().is_some() {
            parent = openat(
                &parent,
                component,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|_| invalid())?;
        }
    }
    Ok(parent)
}

#[cfg(unix)]
fn read_regular(
    parent: &rustix::fd::OwnedFd,
    name: &str,
    mode: &str,
    object: &str,
) -> Result<(u64, String), OrchestratorError> {
    use rustix::fs::{Mode, OFlags, openat};
    use sha1_checked::Digest as _;
    use sha2::Digest as _;
    use std::{io::Read, os::unix::fs::MetadataExt};
    let fd = openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .map_err(|_| invalid())?;
    let mut file = std::fs::File::from(fd);
    let before = file.metadata().map_err(|_| invalid())?;
    if !before.is_file()
        || before.len() > 256 * 1024 * 1024
        || (before.mode() & 0o111 != 0) != (mode == "100755")
        || !matches!(mode, "100644" | "100755")
    {
        return Err(invalid());
    }
    let mut sha1 = sha1_checked::Sha1::new();
    let mut sha256 = sha2::Sha256::new();
    sha1.update(format!("blob {}\0", before.len()).as_bytes());
    let mut count = 0_u64;
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let n = file.read(&mut buffer).map_err(|_| invalid())?;
        if n == 0 {
            break;
        }
        count = count
            .checked_add(u64::try_from(n).map_err(|_| invalid())?)
            .ok_or_else(invalid)?;
        if count > before.len() {
            return Err(invalid());
        }
        sha1.update(&buffer[..n]);
        sha256.update(&buffer[..n]);
    }
    let after = file.metadata().map_err(|_| invalid())?;
    if count != before.len()
        || before.len() != after.len()
        || before.mode() != after.mode()
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
        || before.ctime() != after.ctime()
        || before.ctime_nsec() != after.ctime_nsec()
    {
        return Err(invalid());
    }
    let result = sha1.try_finalize();
    if result.has_collision() || hex(result.hash()) != object {
        return Err(invalid());
    }
    Ok((count, hex(&sha256.finalize())))
}

#[cfg(unix)]
fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(char::from(HEX[usize::from(byte >> 4)]));
        result.push(char::from(HEX[usize::from(byte & 15)]));
    }
    result
}

pub(crate) struct InventoryRoot {
    #[cfg(unix)]
    fd: rustix::fd::OwnedFd,
}

#[cfg(unix)]
pub(crate) fn open_root(root: &Path) -> Result<InventoryRoot, OrchestratorError> {
    let canonical = root.canonicalize().map_err(|_| invalid())?;
    let fd = crate::crate_release_tool_download::open_download_parent(&canonical)?;
    Ok(InventoryRoot { fd })
}

#[cfg(not(unix))]
pub(crate) fn open_root(_root: &Path) -> Result<InventoryRoot, OrchestratorError> {
    Err(invalid())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn rejects_symlink_parent_and_anchors_renamed_parent() -> Result<(), Box<dyn std::error::Error>>
    {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("root");
        std::fs::create_dir_all(root.join("dir"))?;
        std::fs::write(root.join("dir/file"), "abc")?;
        let anchor = open_root(&root)?;
        let parent = open_parent(&anchor, "dir/file")?;
        std::fs::rename(root.join("dir"), root.join("old"))?;
        std::fs::create_dir(temp.path().join("outside"))?;
        std::fs::write(temp.path().join("outside/file"), "outside")?;
        symlink(temp.path().join("outside"), root.join("dir"))?;
        let sha = "f2ba8f84ab5c1bce84a7b441cb1959cfc7093b7f";
        let (_, digest) = read_regular(&parent, "file", "100644", sha)?;
        assert_eq!(
            digest,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(read_inventory(&anchor, "dir/file", "100644", sha).is_err());
        Ok(())
    }
    #[cfg(unix)]
    #[test]
    fn root_anchor_survives_root_path_replacement() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("root");
        std::fs::create_dir_all(root.join("dir"))?;
        std::fs::write(root.join("dir/file"), "abc")?;
        let anchor = open_root(&root)?;
        std::fs::rename(&root, temp.path().join("old"))?;
        std::fs::create_dir_all(root.join("dir"))?;
        std::fs::write(root.join("dir/file"), "outside")?;
        let sha = "f2ba8f84ab5c1bce84a7b441cb1959cfc7093b7f";
        let (_, digest, _) = read_inventory(&anchor, "dir/file", "100644", sha)?;
        assert_eq!(
            digest,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        Ok(())
    }
}
