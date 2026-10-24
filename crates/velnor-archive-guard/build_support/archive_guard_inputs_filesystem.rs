use rustix::fs::{self as ufs, AtFlags, Dir, FileType, Mode, OFlags, Stat};
use std::error::Error;
use std::ffi::{CStr, CString};
use std::fs::File;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use super::{DIRECTORY_COUNT_LIMIT, DISCOVERED_ENTRY_LIMIT, FILE_COUNT_LIMIT};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Identity {
    device: i128,
    inode: i128,
    size: i128,
    mtime: (i128, i128),
    ctime: (i128, i128),
    mode: i128,
    links: i128,
    uid: i128,
}

impl Identity {
    fn from_stat(stat: &Stat) -> Self {
        Self {
            device: i128::from(stat.st_dev),
            inode: i128::from(stat.st_ino),
            size: i128::from(stat.st_size),
            mtime: (i128::from(stat.st_mtime), i128::from(stat.st_mtime_nsec)),
            ctime: (i128::from(stat.st_ctime), i128::from(stat.st_ctime_nsec)),
            mode: i128::from(stat.st_mode),
            links: i128::from(stat.st_nlink),
            uid: i128::from(stat.st_uid),
        }
    }
}

pub(super) fn collect_tree_files_from(
    root: &File,
    relative: &str,
) -> Result<Vec<String>, Box<dyn Error>> {
    let path = safe_relative(relative)?;
    let directory = open_directory(root, &path)?;
    validate_directory(&directory, true)?;
    let mut files = Vec::new();
    let mut directories = 0;
    let mut discovered = 0;
    walk_directory(
        root,
        &directory,
        &path,
        &mut files,
        &mut directories,
        &mut discovered,
    )?;
    files.sort();
    Ok(files)
}

fn walk_directory(
    root: &File,
    directory: &File,
    relative: &Path,
    files: &mut Vec<String>,
    directories: &mut usize,
    discovered: &mut usize,
) -> Result<(), Box<dyn Error>> {
    *directories += 1;
    if *directories > DIRECTORY_COUNT_LIMIT {
        return Err(invalid("source tree directory count limit exceeded").into());
    }
    verify_directory_identity(root, relative, directory)?;
    let mut entries = Vec::new();
    for entry in Dir::read_from(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        if name.to_bytes() == b"." || name.to_bytes() == b".." {
            continue;
        }
        *discovered += 1;
        if *discovered > DISCOVERED_ENTRY_LIMIT {
            return Err(invalid("source tree entry count limit exceeded").into());
        }
        let name = name
            .to_str()
            .map_err(|_| invalid("source path is not UTF-8"))?;
        entries.push((name.to_owned(), relative.join(name)));
    }
    for (name, child_path) in entries {
        let name_c = c_string(&name)?;
        let stat = ufs::statat(directory, name_c.as_c_str(), AtFlags::SYMLINK_NOFOLLOW)?;
        match FileType::from_raw_mode(stat.st_mode) {
            FileType::Directory => {
                let child = openat_directory(directory, &name_c)?;
                validate_directory(&child, true)?;
                if Identity::from_stat(&stat) != Identity::from_stat(&ufs::fstat(&child)?) {
                    return Err(invalid("source tree directory changed during verification").into());
                }
                walk_directory(root, &child, &child_path, files, directories, discovered)?;
            }
            FileType::RegularFile => {
                validate_file_stat(&stat)?;
                if files.len() >= FILE_COUNT_LIMIT {
                    return Err(invalid("source file count limit exceeded").into());
                }
                files.push(
                    child_path
                        .to_str()
                        .ok_or_else(|| invalid("source path is not UTF-8"))?
                        .replace(std::path::MAIN_SEPARATOR, "/"),
                );
            }
            _ => return Err(invalid("source tree contains a non-file entry").into()),
        }
    }
    verify_directory_identity(root, relative, directory)?;
    Ok(())
}

pub(super) fn safe_relative(path: &str) -> Result<PathBuf, Box<dyn Error>> {
    let value = Path::new(path);
    if path.is_empty()
        || path.contains('\\')
        || value.is_absolute()
        || value
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(invalid("archive input path is not repository-relative").into());
    }
    Ok(value.to_path_buf())
}

pub(super) fn read_file(
    root: &File,
    relative: &str,
    limit: u64,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let path = safe_relative(relative)?;
    let name = path
        .file_name()
        .ok_or_else(|| invalid("archive input path has no filename"))?;
    let parent = open_directory(root, path.parent().unwrap_or_else(|| Path::new("")))?;
    validate_directory(&parent, true)?;
    let name = c_string(
        name.to_str()
            .ok_or_else(|| invalid("source path is not UTF-8"))?,
    )?;
    let fd = ufs::openat(
        &parent,
        name.as_c_str(),
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    let before = ufs::fstat(&fd)?;
    validate_file_stat(&before)?;
    if u64::try_from(before.st_size)? > limit {
        return Err(invalid("archive input file is unsafe or too large").into());
    }
    let named = ufs::statat(&parent, name.as_c_str(), AtFlags::SYMLINK_NOFOLLOW)?;
    if Identity::from_stat(&before) != Identity::from_stat(&named) {
        return Err(invalid("archive input file changed while opening").into());
    }
    let mut file = File::from(fd);
    let mut bytes = Vec::new();
    file.by_ref()
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)?;
    let after = ufs::fstat(&file)?;
    let named = ufs::statat(&parent, name.as_c_str(), AtFlags::SYMLINK_NOFOLLOW)?;
    if Identity::from_stat(&before) != Identity::from_stat(&after)
        || Identity::from_stat(&before) != Identity::from_stat(&named)
        || u64::try_from(bytes.len())? > limit
    {
        return Err(invalid("archive input file changed while reading").into());
    }
    verify_directory_path(
        root,
        path.parent().unwrap_or_else(|| Path::new("")),
        &parent,
    )?;
    Ok(bytes)
}

pub(super) fn open_root(path: &Path) -> Result<File, Box<dyn Error>> {
    if !path.is_absolute() {
        return Err(invalid("archive checkout path must be absolute").into());
    }
    let mut current = File::from(ufs::open(
        "/",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?);
    validate_ancestor(&current)?;
    let parts = path
        .components()
        .filter_map(|part| match part {
            Component::RootDir | Component::CurDir => None,
            Component::Normal(name) => Some(Ok(name)),
            _ => Some(Err(invalid("archive checkout path has unsafe components"))),
        })
        .collect::<Result<Vec<_>, _>>()?;
    if parts.is_empty() {
        return Err(invalid("archive checkout path cannot be filesystem root").into());
    }
    for (index, part) in parts.iter().enumerate() {
        let name = c_string(
            part.to_str()
                .ok_or_else(|| invalid("archive checkout path is not UTF-8"))?,
        )?;
        let next = openat_directory(&current, &name)?;
        if index + 1 == parts.len() {
            validate_directory(&next, true)?;
        } else {
            validate_ancestor(&next)?;
        }
        let named = ufs::statat(&current, name.as_c_str(), AtFlags::SYMLINK_NOFOLLOW)?;
        if Identity::from_stat(&named) != Identity::from_stat(&ufs::fstat(&next)?) {
            return Err(invalid("archive checkout ancestry changed during verification").into());
        }
        current = next;
    }
    Ok(current)
}

fn open_directory(root: &File, path: &Path) -> Result<File, Box<dyn Error>> {
    let mut current = root.try_clone()?;
    for component in path.components() {
        let Component::Normal(part) = component else {
            return Err(invalid("archive input parent path is unsafe").into());
        };
        let name = c_string(
            part.to_str()
                .ok_or_else(|| invalid("source path is not UTF-8"))?,
        )?;
        let next = openat_directory(&current, &name)?;
        validate_directory(&next, true)?;
        let named = ufs::statat(&current, name.as_c_str(), AtFlags::SYMLINK_NOFOLLOW)?;
        if Identity::from_stat(&named) != Identity::from_stat(&ufs::fstat(&next)?) {
            return Err(invalid("archive input parent changed during verification").into());
        }
        current = next;
    }
    Ok(current)
}

fn openat_directory(parent: &File, name: &CStr) -> Result<File, Box<dyn Error>> {
    let fd = ufs::openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    Ok(File::from(fd))
}

fn validate_file_stat(stat: &Stat) -> Result<(), Box<dyn Error>> {
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile
        || stat.st_nlink != 1
        || stat.st_uid != rustix::process::geteuid().as_raw()
        || stat.st_mode & 0o022 != 0
        || stat.st_size < 0
    {
        return Err(invalid("archive input file has untrusted provenance").into());
    }
    Ok(())
}

fn validate_directory(file: &File, checkout_owned: bool) -> Result<(), Box<dyn Error>> {
    let stat = ufs::fstat(file)?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::Directory
        || stat.st_mode & 0o022 != 0
        || (checkout_owned && stat.st_uid != rustix::process::geteuid().as_raw())
    {
        return Err(invalid("archive input directory has untrusted provenance").into());
    }
    Ok(())
}

fn validate_ancestor(file: &File) -> Result<(), Box<dyn Error>> {
    let stat = ufs::fstat(file)?;
    let owner_ok = stat.st_uid == 0 || stat.st_uid == rustix::process::geteuid().as_raw();
    let writable = stat.st_mode & 0o022 != 0;
    let root_sticky = stat.st_uid == 0 && stat.st_mode & 0o1000 != 0;
    if FileType::from_raw_mode(stat.st_mode) != FileType::Directory
        || !owner_ok
        || (writable && !root_sticky)
    {
        return Err(invalid("archive checkout ancestry has untrusted provenance").into());
    }
    Ok(())
}

fn verify_directory_identity(
    root: &File,
    relative: &Path,
    file: &File,
) -> Result<(), Box<dyn Error>> {
    let held = ufs::fstat(file)?;
    let parent = open_directory(root, relative.parent().unwrap_or_else(|| Path::new("")))?;
    let name = c_string(
        relative
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| invalid("source path is not UTF-8"))?,
    )?;
    let named = ufs::statat(&parent, name.as_c_str(), AtFlags::SYMLINK_NOFOLLOW)?;
    if Identity::from_stat(&held) != Identity::from_stat(&named) {
        return Err(invalid("source tree directory changed during verification").into());
    }
    validate_directory(file, true)
}

fn verify_directory_path(root: &File, relative: &Path, held: &File) -> Result<(), Box<dyn Error>> {
    let reopened = open_directory(root, relative)?;
    if Identity::from_stat(&ufs::fstat(held)?) != Identity::from_stat(&ufs::fstat(&reopened)?) {
        return Err(invalid("archive input parent changed during verification").into());
    }
    Ok(())
}

pub(super) fn verify_root_path(path: &Path, held: &File) -> Result<(), Box<dyn Error>> {
    let reopened = open_root(path)?;
    if Identity::from_stat(&ufs::fstat(held)?) != Identity::from_stat(&ufs::fstat(&reopened)?) {
        return Err(invalid("archive checkout changed during verification").into());
    }
    Ok(())
}

fn c_string(value: &str) -> Result<CString, Box<dyn Error>> {
    Ok(CString::new(value)?)
}
pub(super) fn is_not_found(error: &(dyn Error + 'static)) -> bool {
    error
        .downcast_ref::<io::Error>()
        .is_some_and(|error| error.kind() == io::ErrorKind::NotFound)
        || error
            .downcast_ref::<rustix::io::Errno>()
            .is_some_and(|error| *error == rustix::io::Errno::NOENT)
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
