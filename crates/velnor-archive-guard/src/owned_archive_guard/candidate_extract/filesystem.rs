// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    AtFlags, CString, Component, File, FileIdentity, Mode, OFlags, Ordering, Path, TEMP_ID,
    Temporary, fs, unix_fs,
};

pub(super) fn open_archive(directory: &File, name: &CString) -> Result<File, String> {
    let fd = unix_fs::openat(
        directory,
        name.as_c_str(),
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .map_err(|error| format!("candidate archive is unavailable: {error}"))?;
    Ok(File::from(fd))
}

pub(super) fn open_private_directory(path: &Path) -> Result<File, String> {
    open_directory_path(path, true)
}

pub(super) fn open_trusted_parent(path: &Path) -> Result<(File, CString), String> {
    let name = path
        .file_name()
        .ok_or_else(|| "candidate archive path has no file name".to_owned())?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let directory = open_directory_path(parent, true)?;
    let name = CString::new(name.as_encoded_bytes())
        .map_err(|_| "candidate archive path contains a NUL byte".to_owned())?;
    Ok((directory, name))
}

pub(super) fn open_directory_path(path: &Path, require_private: bool) -> Result<File, String> {
    let Some(name) = path.file_name() else {
        let resolved = fs::canonicalize(path)
            .map_err(|error| format!("candidate directory path is unavailable: {error}"))?;
        return open_absolute_directory(&resolved, require_private);
    };
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = fs::canonicalize(parent)
        .map_err(|error| format!("candidate directory parent is unavailable: {error}"))?;
    let parent = open_absolute_directory(&parent, false)?;
    let directory = unix_fs::openat(
        &parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| format!("candidate directory is unavailable: {error}"))?;
    let metadata = unix_fs::fstat(&directory)
        .map_err(|error| format!("candidate directory metadata unavailable: {error}"))?;
    if require_private {
        validate_private_directory(&metadata)?;
    } else {
        validate_trusted_ancestor(&metadata)?;
    }
    Ok(File::from(directory))
}

pub(super) fn open_absolute_directory(path: &Path, require_private: bool) -> Result<File, String> {
    if !path.is_absolute() {
        return Err("candidate directory path is not absolute".to_owned());
    }
    let mut current = File::from(
        unix_fs::open(
            "/",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| format!("candidate directory root unavailable: {error}"))?,
    );
    let components = path
        .components()
        .filter_map(|component| match component {
            Component::RootDir | Component::CurDir => None,
            Component::Normal(name) => Some(Ok(name)),
            Component::ParentDir | Component::Prefix(_) => Some(Err(
                "candidate directory path has unsafe components".to_owned(),
            )),
        })
        .collect::<Result<Vec<_>, _>>()?;
    if components.is_empty() {
        return Err("candidate output directory cannot be filesystem root".to_owned());
    }
    for (index, component) in components.iter().enumerate() {
        let next = unix_fs::openat(
            &current,
            *component,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| format!("candidate directory component unavailable: {error}"))?;
        let metadata = unix_fs::fstat(&next)
            .map_err(|error| format!("candidate directory metadata unavailable: {error}"))?;
        if index + 1 == components.len() && require_private {
            validate_private_directory(&metadata)?;
        } else {
            validate_trusted_ancestor(&metadata)?;
        }
        current = File::from(next);
    }
    Ok(current)
}

pub(super) fn validate_private_directory(metadata: &rustix::fs::Stat) -> Result<(), String> {
    if unix_fs::FileType::from_raw_mode(metadata.st_mode) != unix_fs::FileType::Directory
        || metadata.st_uid != rustix::process::geteuid().as_raw()
        || (metadata.st_mode & 0o022) != 0
    {
        return Err("candidate output directory is not owned and private".to_owned());
    }
    Ok(())
}

pub(super) fn validate_trusted_ancestor(metadata: &rustix::fs::Stat) -> Result<(), String> {
    let file_type = unix_fs::FileType::from_raw_mode(metadata.st_mode);
    let owner_is_trusted =
        metadata.st_uid == 0 || metadata.st_uid == rustix::process::geteuid().as_raw();
    let shared_writable = (metadata.st_mode & 0o022) != 0;
    let root_sticky = metadata.st_uid == 0 && (metadata.st_mode & 0o1000) != 0;
    if file_type != unix_fs::FileType::Directory
        || !owner_is_trusted
        || (shared_writable && !root_sticky)
    {
        return Err("candidate directory ancestry is untrusted".to_owned());
    }
    Ok(())
}

pub(super) fn identity_from_fd(file: &File) -> Result<FileIdentity, String> {
    unix_fs::fstat(file)
        .map(|metadata| file_identity(&metadata))
        .map_err(|error| format!("candidate temporary file metadata unavailable: {error}"))
}

pub(super) fn stat_at(directory: &File, name: &CString) -> rustix::io::Result<rustix::fs::Stat> {
    unix_fs::statat(directory, name.as_c_str(), AtFlags::SYMLINK_NOFOLLOW)
}

pub(super) fn named_file_matches(
    directory: &File,
    name: &CString,
    identity: FileIdentity,
    expected_links: i128,
    allow_non_regular: bool,
    exact_snapshot: bool,
) -> bool {
    stat_at(directory, name).is_ok_and(|metadata| {
        let actual = file_identity(&metadata);
        (allow_non_regular
            || unix_fs::FileType::from_raw_mode(metadata.st_mode) == unix_fs::FileType::RegularFile)
            && actual.same_object(identity)
            && actual.links == expected_links
            && (!exact_snapshot || actual.same_snapshot(identity))
    })
}

pub(super) fn unlink_matching(
    directory: &File,
    name: &CString,
    identity: FileIdentity,
    expected_links: i128,
    allow_non_regular: bool,
    exact_snapshot: bool,
) {
    let should_unlink = stat_at(directory, name).is_ok_and(|metadata| {
        let actual = file_identity(&metadata);
        (allow_non_regular
            || unix_fs::FileType::from_raw_mode(metadata.st_mode) == unix_fs::FileType::RegularFile)
            && actual.same_inode(identity)
            && (expected_links < 0 || actual.links == expected_links)
            && (!exact_snapshot || actual.same_snapshot(identity))
    });
    if should_unlink {
        let _cleanup_result = unix_fs::unlinkat(directory, name.as_c_str(), AtFlags::empty());
    }
}

pub(super) fn cleanup_temporaries(directory: &File, temporary: &[Temporary]) {
    for entry in temporary {
        unlink_matching(directory, &entry.name, entry.identity, -1, false, false);
    }
}

pub(super) fn create_temp_at(directory: &File) -> Result<Temporary, String> {
    for _ in 0..128 {
        let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let name = c_string(&format!(".velnor-release-{}-{id}", std::process::id()))?;
        match unix_fs::openat(
            directory,
            name.as_c_str(),
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
        ) {
            Ok(fd) => {
                let file = File::from(fd);
                let identity = match identity_from_fd(&file) {
                    Ok(identity) => identity,
                    Err(error) => {
                        let _cleanup_result =
                            unix_fs::unlinkat(directory, name.as_c_str(), AtFlags::empty());
                        return Err(error);
                    }
                };
                return Ok(Temporary {
                    name,
                    file,
                    identity,
                });
            }
            Err(rustix::io::Errno::EXIST) => {}
            Err(error) => {
                return Err(format!("candidate temporary file creation failed: {error}"));
            }
        }
    }
    Err("candidate temporary-name allocation failed".to_owned())
}

pub(super) fn exists_at(directory: &File, name: &str) -> Result<bool, String> {
    let name = c_string(name)?;
    match stat_at(directory, &name) {
        Ok(_) => Ok(true),
        Err(rustix::io::Errno::NOENT) => Ok(false),
        Err(error) => Err(format!("candidate output lookup failed: {error}")),
    }
}

pub(super) fn file_identity(metadata: &rustix::fs::Stat) -> FileIdentity {
    FileIdentity {
        device: i128::from(metadata.st_dev),
        inode: i128::from(metadata.st_ino),
        size: i128::from(metadata.st_size),
        mtime_seconds: i128::from(metadata.st_mtime),
        mtime_nanoseconds: i128::from(metadata.st_mtime_nsec),
        ctime_seconds: i128::from(metadata.st_ctime),
        ctime_nanoseconds: i128::from(metadata.st_ctime_nsec),
        mode: i128::from(metadata.st_mode),
        links: i128::from(metadata.st_nlink),
    }
}

pub(super) fn c_string(value: &str) -> Result<CString, String> {
    CString::new(value).map_err(|_| "candidate path contains a NUL byte".to_owned())
}
