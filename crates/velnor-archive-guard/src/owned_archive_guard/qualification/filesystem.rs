use std::ffi::{CStr, CString};
use std::fs::File;
use std::io::{Read, Take};
use std::path::{Component, Path, PathBuf};

use rustix::fs::{self as unix_fs, AtFlags, Dir, FileType, Mode, OFlags, Stat};

use super::{
    MAX_WORKFLOW_DEPTH, MAX_WORKFLOW_ENTRIES, MAX_WORKFLOW_FILE_BYTES, MAX_WORKFLOW_FILES,
    MAX_WORKFLOW_TREE_BYTES,
};

pub(super) fn read_regular(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let (parent, name) = open_parent(path)?;
    let before = statat(&parent, &name)?;
    if FileType::from_raw_mode(before.st_mode) != FileType::RegularFile {
        return Err(format!(
            "expected regular file no larger than {limit} bytes"
        ));
    }
    read_named(&parent, &name, before, limit)
}

pub(super) struct TreeBytes {
    pub(super) files: Vec<(PathBuf, Vec<u8>)>,
}

pub(super) fn read_tree(root: &Path) -> Result<TreeBytes, String> {
    let directory = open_directory(root)
        .map_err(|_| "candidate manifest or generated workflow tree is incomplete".to_owned())?;
    let initial = unix_fs::fstat(&directory).map_err(|error| error.to_string())?;
    let mut state = TreeState {
        files: Vec::new(),
        total: 0,
        entries: 0,
    };
    walk(&directory, root, 0, &mut state)?;
    let final_stat = unix_fs::fstat(&directory).map_err(|error| error.to_string())?;
    if !same_snapshot(&initial, &final_stat) {
        return Err("generated workflow root changed during traversal".into());
    }
    Ok(TreeBytes { files: state.files })
}

struct TreeState {
    files: Vec<(PathBuf, Vec<u8>)>,
    total: u64,
    entries: usize,
}

fn walk(directory: &File, path: &Path, depth: usize, state: &mut TreeState) -> Result<(), String> {
    if depth > MAX_WORKFLOW_DEPTH {
        return Err(format!(
            "generated workflow tree exceeds {MAX_WORKFLOW_DEPTH} directory levels"
        ));
    }
    let initial = unix_fs::fstat(directory).map_err(|error| error.to_string())?;
    let fd = rustix::io::dup(directory).map_err(|error| error.to_string())?;
    let mut entries = Dir::read_from(fd).map_err(|error| error.to_string())?;
    while let Some(entry) = entries.read() {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name();
        if name.to_bytes() == b"." || name.to_bytes() == b".." {
            continue;
        }
        visit_entry(directory, path, depth, state, name)?;
    }
    let final_stat = unix_fs::fstat(directory).map_err(|error| error.to_string())?;
    if !same_snapshot(&initial, &final_stat) {
        return Err("generated workflow directory changed during traversal".into());
    }
    Ok(())
}

fn visit_entry(
    directory: &File,
    path: &Path,
    depth: usize,
    state: &mut TreeState,
    name: &CStr,
) -> Result<(), String> {
    state.entries = state
        .entries
        .checked_add(1)
        .filter(|count| *count <= MAX_WORKFLOW_ENTRIES)
        .ok_or_else(|| {
            format!("generated workflow tree exceeds {MAX_WORKFLOW_ENTRIES} total entries")
        })?;
    let metadata = statat(directory, name)?;
    match FileType::from_raw_mode(metadata.st_mode) {
        FileType::Directory => {
            let child = unix_fs::openat(
                directory,
                name,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|error| error.to_string())?;
            let child = File::from(child);
            let opened = unix_fs::fstat(&child).map_err(|error| error.to_string())?;
            if !same_object(&metadata, &opened) {
                return Err("generated workflow directory changed during traversal".into());
            }
            walk(
                &child,
                &path.join(name.to_string_lossy().as_ref()),
                depth + 1,
                state,
            )?;
        }
        FileType::RegularFile => read_tree_file(directory, path, name, metadata, state)?,
        _ => {}
    }
    Ok(())
}

fn read_tree_file(
    directory: &File,
    path: &Path,
    name: &CStr,
    metadata: Stat,
    state: &mut TreeState,
) -> Result<(), String> {
    if state.files.len() == MAX_WORKFLOW_FILES {
        return Err(format!(
            "generated workflow tree exceeds {MAX_WORKFLOW_FILES} regular files"
        ));
    }
    let remaining = MAX_WORKFLOW_TREE_BYTES.saturating_sub(state.total);
    let advertised_size = u64::try_from(metadata.st_size)
        .map_err(|_| "generated workflow file has a negative size".to_owned())?;
    if advertised_size > remaining {
        return Err(format!(
            "generated workflow tree exceeds {MAX_WORKFLOW_TREE_BYTES} bytes"
        ));
    }
    let file_limit = MAX_WORKFLOW_FILE_BYTES.min(remaining);
    let bytes = read_named(directory, name, metadata, file_limit).map_err(|error| {
        format!(
            "cannot read generated workflow {}: {error}",
            path.join(name.to_string_lossy().as_ref()).display()
        )
    })?;
    state.total = state
        .total
        .checked_add(
            u64::try_from(bytes.len())
                .map_err(|_| "generated workflow byte count overflow".to_owned())?,
        )
        .filter(|total| *total <= MAX_WORKFLOW_TREE_BYTES)
        .ok_or_else(|| {
            format!("generated workflow tree exceeds {MAX_WORKFLOW_TREE_BYTES} bytes")
        })?;
    state
        .files
        .push((path.join(name.to_string_lossy().as_ref()), bytes));
    Ok(())
}

fn read_named(directory: &File, name: &CStr, before: Stat, limit: u64) -> Result<Vec<u8>, String> {
    let initial_size = u64::try_from(before.st_size)
        .map_err(|_| "expected regular file with a nonnegative size".to_owned())?;
    if initial_size > limit {
        return Err(format!(
            "expected regular file no larger than {limit} bytes"
        ));
    }
    let fd = unix_fs::openat(
        directory,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| error.to_string())?;
    let opened = unix_fs::fstat(&fd).map_err(|error| error.to_string())?;
    if FileType::from_raw_mode(opened.st_mode) != FileType::RegularFile
        || !same_object(&before, &opened)
    {
        return Err("regular file changed during read".into());
    }
    let mut file = File::from(fd);
    let cap = limit
        .checked_add(1)
        .ok_or_else(|| "file limit overflow".to_owned())?;
    let mut bytes = Vec::new();
    let mut reader: Take<&mut File> = (&mut file).take(cap);
    reader
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    let actual_size =
        u64::try_from(bytes.len()).map_err(|_| "regular file byte count overflow".to_owned())?;
    if actual_size > limit {
        return Err(format!(
            "expected regular file no larger than {limit} bytes"
        ));
    }
    let after = unix_fs::fstat(&file).map_err(|error| error.to_string())?;
    let named_after = statat(directory, name)?;
    let final_size =
        u64::try_from(after.st_size).map_err(|_| "regular file changed during read".to_owned())?;
    if !same_object(&opened, &after)
        || !same_object(&opened, &named_after)
        || final_size != actual_size
    {
        return Err("regular file changed during read".into());
    }
    Ok(bytes)
}

fn open_parent(path: &Path) -> Result<(File, CString), String> {
    let name = path
        .file_name()
        .ok_or_else(|| "path has no file name".to_owned())?;
    let parent_path = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = open_directory(parent_path)?;
    let name = CString::new(name.as_encoded_bytes()).map_err(|_| "path contains NUL".to_owned())?;
    Ok((parent, name))
}

fn open_directory(path: &Path) -> Result<File, String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join(path)
    };
    let mut current = File::from(
        unix_fs::open(
            "/",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| error.to_string())?,
    );
    for component in absolute.components() {
        let part = match component {
            Component::RootDir | Component::CurDir => continue,
            Component::Normal(part) => part,
            Component::ParentDir => {
                current = File::from(
                    unix_fs::openat(
                        &current,
                        "..",
                        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                        Mode::empty(),
                    )
                    .map_err(|error| error.to_string())?,
                );
                continue;
            }
            Component::Prefix(_) => return Err("path prefix is not supported".into()),
        };
        let name = CString::new(part.as_encoded_bytes())
            .map_err(|_| "path component contains NUL".to_owned())?;
        current = File::from(
            unix_fs::openat(
                &current,
                name.as_c_str(),
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|error| error.to_string())?,
        );
    }
    Ok(current)
}

fn statat(directory: &File, name: &CStr) -> Result<Stat, String> {
    unix_fs::statat(directory, name, AtFlags::SYMLINK_NOFOLLOW).map_err(|error| error.to_string())
}

fn same_object(left: &Stat, right: &Stat) -> bool {
    left.st_dev == right.st_dev && left.st_ino == right.st_ino
}

fn same_snapshot(left: &Stat, right: &Stat) -> bool {
    same_object(left, right)
        && left.st_size == right.st_size
        && left.st_mtime == right.st_mtime
        && left.st_mtime_nsec == right.st_mtime_nsec
        && left.st_ctime == right.st_ctime
        && left.st_ctime_nsec == right.st_ctime_nsec
}
