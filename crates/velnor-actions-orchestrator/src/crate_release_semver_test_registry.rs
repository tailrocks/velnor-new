// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use rustix::fs::{self as rfs, AtFlags, Dir, FileType, Mode, OFlags};
use std::os::unix::fs::MetadataExt as _;
use std::{
    fs,
    io::{Read, Write},
    os::fd::OwnedFd,
    path::{Component, Path},
};

const FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);
const MAX_FILES: usize = 100_000;
const MAX_BYTES: u64 = 512 * 1024 * 1024;

pub(super) fn copy_snapshot(source: &Path, home: &Path) -> Result<(), &'static str> {
    copy_snapshot_with(source, home, || {})
}

fn copy_snapshot_with(
    source: &Path,
    home: &Path,
    after_open: impl FnOnce(),
) -> Result<(), &'static str> {
    let invalid = || "fixture_registry_invalid";
    if !source.is_absolute() || !home.is_absolute() {
        return Err(invalid());
    }
    let mut fd = rfs::open("/", FLAGS | OFlags::DIRECTORY, Mode::empty()).map_err(|_| invalid())?;
    for part in source.components() {
        match part {
            Component::RootDir => {}
            Component::Normal(name) => {
                fd = rfs::openat(&fd, name, FLAGS | OFlags::DIRECTORY, Mode::empty())
                    .map_err(|_| invalid())?;
            }
            _ => return Err(invalid()),
        }
    }
    after_open();
    let dest = home.join("registry");
    fs::create_dir(&dest).map_err(|_| invalid())?;
    let mut count = 0;
    let mut bytes = 0;
    let mut roots = std::collections::BTreeSet::new();
    for entry in Dir::read_from(&fd).map_err(|_| invalid())? {
        let entry = entry.map_err(|_| invalid())?;
        let name = entry.file_name().to_str().map_err(|_| invalid())?;
        if name == "." || name == ".." {
            continue;
        }
        if !roots.insert(name.to_owned()) || !matches!(name, "index" | "cache") {
            return Err(invalid());
        }
        let child = rfs::openat(&fd, name, FLAGS | OFlags::DIRECTORY, Mode::empty())
            .map_err(|_| invalid())?;
        let path = dest.join(name);
        fs::create_dir(&path).map_err(|_| invalid())?;
        copy_dir(&child, &path, 1, &mut count, &mut bytes)?;
    }
    if roots != std::collections::BTreeSet::from(["cache".to_owned(), "index".to_owned()]) {
        return Err(invalid());
    }
    Ok(())
}

fn copy_dir(
    source: &OwnedFd,
    dest: &Path,
    depth: usize,
    count: &mut usize,
    total: &mut u64,
) -> Result<(), &'static str> {
    let invalid = || "fixture_registry_invalid";
    if depth > 32 {
        return Err(invalid());
    }
    for entry in Dir::read_from(source).map_err(|_| invalid())? {
        let entry = entry.map_err(|_| invalid())?;
        let name = entry.file_name().to_str().map_err(|_| invalid())?;
        if name == "." || name == ".." {
            continue;
        }
        if name.contains('\\')
            || name.bytes().any(|b| b.is_ascii_control())
            || matches!(
                name,
                "credentials" | "credentials.toml" | "config.toml" | "config"
            )
        {
            return Err(invalid());
        }
        *count = count.checked_add(1).ok_or_else(invalid)?;
        if *count > MAX_FILES {
            return Err(invalid());
        }
        let stat = rfs::statat(source, name, AtFlags::SYMLINK_NOFOLLOW).map_err(|_| invalid())?;
        match FileType::from_raw_mode(stat.st_mode) {
            FileType::Directory => {
                let child = rfs::openat(source, name, FLAGS | OFlags::DIRECTORY, Mode::empty())
                    .map_err(|_| invalid())?;
                let path = dest.join(name);
                fs::create_dir(&path).map_err(|_| invalid())?;
                copy_dir(&child, &path, depth + 1, count, total)?;
            }
            FileType::RegularFile => {
                let fd = rfs::openat(source, name, FLAGS | OFlags::NONBLOCK, Mode::empty())
                    .map_err(|_| invalid())?;
                let mut file = fs::File::from(fd);
                let metadata = file.metadata().map_err(|_| invalid())?;
                if !metadata.is_file() || metadata.nlink() != 1 {
                    return Err(invalid());
                }
                *total = total.checked_add(metadata.len()).ok_or_else(invalid)?;
                if *total > MAX_BYTES {
                    return Err(invalid());
                }
                let mut out = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(dest.join(name))
                    .map_err(|_| invalid())?;
                let copied = std::io::copy(&mut (&mut file).take(metadata.len() + 1), &mut out)
                    .map_err(|_| invalid())?;
                out.flush().map_err(|_| invalid())?;
                if copied != metadata.len() {
                    return Err(invalid());
                }
            }
            _ => return Err(invalid()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::{fs::symlink, net::UnixListener};

    fn fixture() -> Result<(tempfile::TempDir, std::path::PathBuf), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?.join("snapshot");
        fs::create_dir(&root)?;
        fs::create_dir(root.join("index"))?;
        fs::create_dir(root.join("cache"))?;
        fs::write(root.join("index/config.json"), b"{}")?;
        fs::write(root.join("cache/example.crate"), b"fixture")?;
        Ok((temp, root))
    }

    #[test]
    fn copies_only_regular_bounded_registry_data() -> Result<(), Box<dyn std::error::Error>> {
        let (temp, root) = fixture()?;
        let home = temp.path().join("home");
        fs::create_dir(&home)?;
        copy_snapshot(&root, &home)?;
        assert_eq!(
            fs::read(home.join("registry/cache/example.crate"))?,
            b"fixture"
        );
        assert_eq!(fs::read(home.join("registry/index/config.json"))?, b"{}");
        assert!(!home.join("credentials.toml").exists());
        Ok(())
    }

    #[test]
    fn rejects_links_special_files_and_unsafe_names() -> Result<(), Box<dyn std::error::Error>> {
        for mode in 0..6 {
            let (temp, root) = fixture()?;
            let home = temp.path().join("home");
            fs::create_dir(&home)?;
            match mode {
                0 => symlink(root.join("cache/example.crate"), root.join("index/link"))?,
                1 => symlink(root.join("cache"), root.join("index/link"))?,
                2 => {
                    let _socket = UnixListener::bind(root.join("index/socket"))?;
                    assert!(copy_snapshot(&root, &home).is_err());
                    continue;
                }
                3 => fs::hard_link(root.join("cache/example.crate"), root.join("index/hard"))?,
                4 => fs::write(root.join("index/credentials.toml"), "fixture")?,
                5 => {
                    let link = temp.path().join("alias");
                    symlink(&root, &link)?;
                    assert!(copy_snapshot(&link, &home).is_err());
                    continue;
                }
                _ => unreachable!(),
            }
            assert!(copy_snapshot(&root, &home).is_err());
        }
        Ok(())
    }
    #[test]
    fn source_swap_cannot_redirect_open_snapshot() -> Result<(), Box<dyn std::error::Error>> {
        let (temp, root) = fixture()?;
        let home = temp.path().join("home");
        fs::create_dir(&home)?;
        let outside = temp.path().join("outside");
        fs::create_dir(&outside)?;
        fs::write(outside.join("secret"), b"fixture")?;
        let moved = temp.path().join("moved");
        copy_snapshot_with(&root, &home, || {
            fs::rename(&root, &moved).expect("rename fixture root");
            symlink(&outside, &root).expect("replace fixture root");
        })?;
        assert_eq!(
            fs::read(home.join("registry/cache/example.crate"))?,
            b"fixture"
        );
        assert!(!home.join("registry/secret").exists());
        Ok(())
    }
}
