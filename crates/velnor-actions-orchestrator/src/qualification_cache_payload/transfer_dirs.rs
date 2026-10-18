use super::stage::ensure_child_directory;
use rustix::fs::{AtFlags, FileType, Mode, OFlags, Stat, open, openat, statat};
use std::ffi::CString;
use std::ffi::OsStr;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path};

pub(super) fn ensure_owned_directory_path(
    path: &Path,
    runner_temp: &Path,
) -> Result<(), &'static str> {
    ensure_owned_directory_path_from(path, runner_temp)
}

pub(super) fn ensure_owned_directory_path_from(
    path: &Path,
    anchor: &Path,
) -> Result<(), &'static str> {
    let relative = path
        .strip_prefix(anchor)
        .map_err(|_| "qualification_cache_root_path_invalid")?;
    let mut fd = open_owned_directory(anchor)?;
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err("qualification_cache_root_path_invalid");
        };
        let name = c_name(name)?;
        fd = ensure_child_directory(&fd, name.as_c_str())?;
    }
    Ok(())
}

pub(super) fn open_directory_path(path: &Path) -> Result<rustix::fd::OwnedFd, &'static str> {
    if !path.is_absolute() {
        return Err("qualification_cache_path_invalid");
    }
    let mut fd = open(
        "/",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| "qualification_cache_path_open_failed")?;
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => {
                let name = c_name(name)?;
                fd = open_directory_at(&fd, name.as_c_str())?;
            }
            Component::CurDir | Component::ParentDir | Component::Prefix(_) => {
                return Err("qualification_cache_path_invalid");
            }
        }
    }
    Ok(fd)
}

pub(super) fn open_owned_directory(path: &Path) -> Result<rustix::fd::OwnedFd, &'static str> {
    let fd = open_directory_path(path)?;
    validate_owned_directory(&fd)?;
    Ok(fd)
}

pub(super) fn open_directory_at(
    parent: &rustix::fd::OwnedFd,
    name: &std::ffi::CStr,
) -> Result<rustix::fd::OwnedFd, &'static str> {
    openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| "qualification_cache_directory_open_failed")
}

pub(super) fn open_parent(path: &Path) -> Result<(rustix::fd::OwnedFd, CString), &'static str> {
    let parent = path.parent().ok_or("qualification_cache_path_invalid")?;
    let name = path.file_name().ok_or("qualification_cache_path_invalid")?;
    Ok((open_directory_path(parent)?, c_name(name)?))
}

pub(super) fn stat_optional(
    parent: &rustix::fd::OwnedFd,
    name: &std::ffi::CStr,
) -> Result<Option<Stat>, &'static str> {
    match statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) => Ok(Some(stat)),
        Err(error) if error == rustix::io::Errno::NOENT => Ok(None),
        Err(_) => Err("qualification_cache_path_stat_failed"),
    }
}

pub(super) fn path_exists_no_follow(path: &Path) -> Result<bool, &'static str> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err("qualification_cache_path_stat_failed"),
    }
}

pub(super) fn validate_owned_directory(fd: &rustix::fd::OwnedFd) -> Result<(), &'static str> {
    let stat = rustix::fs::fstat(fd).map_err(|_| "qualification_cache_directory_stat_failed")?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::Directory
        || stat.st_uid != rustix::process::geteuid().as_raw()
        || stat.st_mode & 0o7000 != 0
    {
        return Err("qualification_cache_directory_owner_or_mode_invalid");
    }
    Ok(())
}

pub(super) fn validate_file_stat(stat: &Stat) -> Result<(), &'static str> {
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile
        || stat.st_uid != rustix::process::geteuid().as_raw()
        || stat.st_nlink != 1
        || stat.st_mode & 0o7000 != 0
    {
        return Err("qualification_cache_file_owner_kind_or_mode_invalid");
    }
    Ok(())
}

pub(super) fn path_points_to(path: &Path, fd: &rustix::fd::OwnedFd) -> bool {
    let Ok(current) = open_directory_path(path) else {
        return false;
    };
    rustix::fs::fstat(fd)
        .is_ok_and(|left| rustix::fs::fstat(&current).is_ok_and(|right| same_object(&left, &right)))
}

pub(super) fn same_version(left: &Stat, right: &Stat) -> bool {
    left.st_dev == right.st_dev
        && left.st_ino == right.st_ino
        && left.st_mode == right.st_mode
        && left.st_uid == right.st_uid
        && left.st_nlink == right.st_nlink
        && left.st_size == right.st_size
        && left.st_mtime == right.st_mtime
        && left.st_mtime_nsec == right.st_mtime_nsec
        && left.st_ctime == right.st_ctime
        && left.st_ctime_nsec == right.st_ctime_nsec
}

pub(super) fn same_object(left: &Stat, right: &Stat) -> bool {
    left.st_dev == right.st_dev && left.st_ino == right.st_ino
}

pub(super) fn c_name(value: &OsStr) -> Result<CString, &'static str> {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.iter().any(|byte| matches!(*byte, b'/' | b'\0')) {
        return Err("qualification_cache_path_invalid");
    }
    CString::new(bytes).map_err(|_| "qualification_cache_path_invalid")
}
