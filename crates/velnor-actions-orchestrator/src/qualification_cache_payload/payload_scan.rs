//! Walk a payload tree through no-follow directory descriptors.

use rustix::fs::{AtFlags, Dir, FileType, Mode, OFlags, openat, statat};

use super::{Entry, MAX_DEPTH, MAX_ENTRIES, MAX_PATH_BYTES, Tree};

pub(super) fn collect(
    directory_fd: &rustix::fd::OwnedFd,
    relative: &str,
    depth: usize,
    tree: &mut Tree,
) -> Result<(), &'static str> {
    if depth > MAX_DEPTH {
        return Err("qualification_cache_payload_limit");
    }
    let start_stat =
        rustix::fs::fstat(directory_fd).map_err(|_| "qualification_cache_payload_read_failed")?;
    super::validate_directory(&start_stat)?;
    for name in child_names(directory_fd)? {
        collect_child(directory_fd, &name, relative, depth, tree)?;
    }
    let end_stat =
        rustix::fs::fstat(directory_fd).map_err(|_| "qualification_cache_payload_read_failed")?;
    if !super::same_version(&start_stat, &end_stat) {
        return Err("qualification_cache_payload_file_changed");
    }
    Ok(())
}

fn child_names(directory_fd: &rustix::fd::OwnedFd) -> Result<Vec<std::ffi::CString>, &'static str> {
    let directory =
        Dir::read_from(directory_fd).map_err(|_| "qualification_cache_payload_read_failed")?;
    let mut names = Vec::new();
    for entry in directory {
        if names.len() >= MAX_ENTRIES {
            return Err("qualification_cache_payload_limit");
        }
        names.push(
            entry
                .map_err(|_| "qualification_cache_payload_read_failed")?
                .file_name()
                .to_owned(),
        );
    }
    names.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    Ok(names)
}

fn collect_child(
    directory_fd: &rustix::fd::OwnedFd,
    name: &std::ffi::CString,
    parent: &str,
    depth: usize,
    tree: &mut Tree,
) -> Result<(), &'static str> {
    let name_text = name
        .to_str()
        .map_err(|_| "qualification_cache_payload_path_invalid")?;
    if name_text == "." || name_text == ".." {
        return Ok(());
    }
    if name_text.is_empty()
        || name_text
            .bytes()
            .any(|byte| byte < 0x20 || byte == 0x7f || byte == b'/')
    {
        return Err("qualification_cache_payload_path_invalid");
    }
    let relative = if parent.is_empty() {
        name_text.to_owned()
    } else {
        format!("{parent}/{name_text}")
    };
    if relative.len() > MAX_PATH_BYTES || tree.entries.len() >= MAX_ENTRIES {
        return Err("qualification_cache_payload_limit");
    }
    let stat = statat(directory_fd, name.as_c_str(), AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|_| "qualification_cache_payload_read_failed")?;
    super::validate_entry_owner_mode(&stat)?;
    match FileType::from_raw_mode(stat.st_mode) {
        FileType::Directory => collect_directory(directory_fd, name, &relative, depth, &stat, tree),
        FileType::Symlink => {
            let entry = super::hash_symlink(directory_fd, name, relative, &stat, tree)?;
            tree.entries.push(entry);
            Ok(())
        }
        FileType::RegularFile => {
            let entry = super::hash_file(directory_fd, name, relative, &stat, tree)?;
            tree.entries.push(entry);
            Ok(())
        }
        _ => Err("qualification_cache_payload_entry_unsupported"),
    }
}

fn collect_directory(
    parent_fd: &rustix::fd::OwnedFd,
    name: &std::ffi::CString,
    relative: &str,
    depth: usize,
    stat: &rustix::fs::Stat,
    tree: &mut Tree,
) -> Result<(), &'static str> {
    let child_fd = openat(
        parent_fd,
        name.as_c_str(),
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| "qualification_cache_payload_open_failed")?;
    let opened =
        rustix::fs::fstat(&child_fd).map_err(|_| "qualification_cache_payload_open_failed")?;
    if !super::same_version(stat, &opened) {
        return Err("qualification_cache_payload_file_changed");
    }
    tree.entries.push(Entry {
        path: relative.to_owned(),
        kind: b'd',
        mode: u32::from(stat.st_mode & 0o777),
        size: 0,
        content_digest: [0; 32],
        file_identity: None,
    });
    collect(&child_fd, relative, depth + 1, tree)
}
