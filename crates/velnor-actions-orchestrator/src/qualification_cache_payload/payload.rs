use std::fs;
use std::io::Read;
use std::os::fd::AsFd;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use rustix::fs::{AtFlags, FileType, Mode, OFlags, Stat, open, openat, statat};

const DOMAIN: &[u8] = b"velnor-qualification-cache-useful-state\0";
const MAX_ENTRIES: usize = 250_000;
const MAX_TOTAL_BYTES: u64 = 16 * 1024 * 1024 * 1024;
const MAX_PATH_BYTES: usize = 4_096;
const MAX_DEPTH: usize = 128;

#[path = "payload_scan.rs"]
mod scan;

#[derive(Debug, Clone)]
struct Entry {
    path: String,
    kind: u8,
    mode: u32,
    size: u64,
    content_digest: [u8; 32],
    file_identity: Option<(u64, u64)>,
    link_target: Option<String>,
    stat: Stat,
}

#[derive(Default)]
struct Tree {
    entries: Vec<Entry>,
    bytes: u64,
    file_links: std::collections::BTreeMap<(u64, u64), (u64, u64)>,
}

pub(crate) fn digest_stage(root: &Path) -> Result<Option<String>, &'static str> {
    let metadata = fs::symlink_metadata(root).map_err(|_| "qualification_cache_payload_missing")?;
    if !metadata.file_type().is_dir() {
        return Err("qualification_cache_payload_root_invalid");
    }
    let root_fd = open(
        root,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| "qualification_cache_payload_root_invalid")?;
    let root_stat = rustix::fs::fstat(root_fd.as_fd())
        .map_err(|_| "qualification_cache_payload_open_failed")?;
    validate_directory(&root_stat)?;
    if !same_identity(&metadata, &root_stat) {
        return Err("qualification_cache_payload_root_invalid");
    }
    digest_open_directory(&root_fd)
}

fn digest_open_directory(root_fd: &rustix::fd::OwnedFd) -> Result<Option<String>, &'static str> {
    let root_stat =
        rustix::fs::fstat(root_fd).map_err(|_| "qualification_cache_payload_open_failed")?;
    validate_directory(&root_stat)?;
    validate_payload_root_mode(&root_stat)?;
    let mut tree = Tree::default();
    scan::collect(root_fd, "", 0, &mut tree)?;
    if tree.entries.is_empty() {
        return Ok(None);
    }
    if tree
        .file_links
        .values()
        .any(|(expected, observed)| expected != observed)
    {
        return Err("qualification_cache_payload_hardlink_escape");
    }
    Ok(Some(
        inventory::inventory_from_tree(root_stat, tree)?.digest,
    ))
}

#[path = "payload_inventory.rs"]
mod inventory;
pub(crate) use inventory::{
    ArchiveEntry, ArchiveEntryKind, inventory_directory, open_inventory_file,
    validate_inventory_directory, validate_inventory_symlink,
};

fn payload_format_version() -> u8 {
    velnor_actions_contract::workflow::QUALIFICATION_CACHE_FORMAT_VERSION
}

pub(super) fn validate_payload_root_mode(stat: &Stat) -> Result<(), &'static str> {
    if stat.st_mode & 0o7000 != 0 || stat.st_mode & 0o022 != 0 {
        return Err("qualification_cache_payload_owner_or_mode_invalid");
    }
    Ok(())
}

fn validate_directory(stat: &Stat) -> Result<(), &'static str> {
    if FileType::from_raw_mode(stat.st_mode) != FileType::Directory {
        return Err("qualification_cache_payload_root_invalid");
    }
    validate_entry_owner_mode(stat)
}

fn validate_entry_owner_mode(stat: &Stat) -> Result<(), &'static str> {
    if stat.st_mode & 0o7000 != 0 || stat.st_uid != crate::process_owner::effective_uid() {
        return Err("qualification_cache_payload_owner_or_mode_invalid");
    }
    Ok(())
}

fn hash_file(
    parent_fd: &rustix::fd::OwnedFd,
    name: &std::ffi::CString,
    relative: String,
    metadata: &Stat,
    tree: &mut Tree,
) -> Result<Entry, &'static str> {
    let size = u64::try_from(metadata.st_size).map_err(|_| "qualification_cache_payload_limit")?;
    if size > MAX_TOTAL_BYTES.saturating_sub(tree.bytes) || tree.bytes.checked_add(size).is_none() {
        return Err("qualification_cache_payload_limit");
    }
    let fd = openat(
        parent_fd,
        name.as_c_str(),
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| "qualification_cache_payload_open_failed")?;
    let stat = rustix::fs::fstat(&fd).map_err(|_| "qualification_cache_payload_open_failed")?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile
        || !same_version(metadata, &stat)
    {
        return Err("qualification_cache_payload_file_changed");
    }
    let identity = file_identity(metadata)?;
    let links = tree
        .file_links
        .entry(identity)
        .or_insert((u64::from(metadata.st_nlink), 0));
    if links.0 != u64::from(metadata.st_nlink) {
        return Err("qualification_cache_payload_file_changed");
    }
    links.1 = links
        .1
        .checked_add(1)
        .ok_or("qualification_cache_payload_limit")?;
    let mut file = fs::File::from(fd);
    let mut file_digest = blake3::Hasher::new();
    let mut buffer = vec![0; 64 * 1024].into_boxed_slice();
    let mut read_bytes = 0_u64;
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| "qualification_cache_payload_read_failed")?;
        if count == 0 {
            break;
        }
        read_bytes = read_bytes
            .checked_add(count as u64)
            .ok_or("qualification_cache_payload_limit")?;
        if read_bytes > size || tree.bytes.saturating_add(read_bytes) > MAX_TOTAL_BYTES {
            return Err("qualification_cache_payload_limit");
        }
        file_digest.update(&buffer[..count]);
    }
    let end_stat =
        rustix::fs::fstat(&file).map_err(|_| "qualification_cache_payload_read_failed")?;
    if read_bytes != size || !same_version(&stat, &end_stat) {
        return Err("qualification_cache_payload_file_changed");
    }
    tree.bytes = tree
        .bytes
        .checked_add(size)
        .ok_or("qualification_cache_payload_limit")?;
    Ok(Entry {
        path: relative,
        kind: b'f',
        mode: u32::from(metadata.st_mode & 0o777),
        size,
        content_digest: *file_digest.finalize().as_bytes(),
        file_identity: Some(identity),
        link_target: None,
        stat: *metadata,
    })
}

fn hash_symlink(
    parent_fd: &rustix::fd::OwnedFd,
    name: &std::ffi::CString,
    relative: String,
    metadata: &Stat,
    tree: &mut Tree,
) -> Result<Entry, &'static str> {
    if metadata.st_nlink != 1 {
        return Err("qualification_cache_payload_entry_unsupported");
    }
    let target = rustix::fs::readlinkat(parent_fd, name.as_c_str(), Vec::new())
        .map_err(|_| "qualification_cache_payload_read_failed")?;
    let target = target
        .into_string()
        .map_err(|_| "qualification_cache_payload_path_invalid")?;
    validate_internal_link(&relative, &target)?;
    let size = u64::try_from(target.len()).map_err(|_| "qualification_cache_payload_limit")?;
    if size > MAX_PATH_BYTES as u64
        || size > MAX_TOTAL_BYTES.saturating_sub(tree.bytes)
        || tree.bytes.checked_add(size).is_none()
    {
        return Err("qualification_cache_payload_limit");
    }
    let after = statat(parent_fd, name.as_c_str(), AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|_| "qualification_cache_payload_read_failed")?;
    if !same_version(metadata, &after)
        || size != u64::try_from(metadata.st_size).unwrap_or_default()
    {
        return Err("qualification_cache_payload_file_changed");
    }
    tree.bytes += size;
    Ok(Entry {
        path: relative,
        kind: b'l',
        mode: u32::from(metadata.st_mode & 0o777),
        size,
        content_digest: *blake3::hash(target.as_bytes()).as_bytes(),
        file_identity: None,
        link_target: Some(target),
        stat: *metadata,
    })
}

fn file_identity(stat: &Stat) -> Result<(u64, u64), &'static str> {
    Ok((
        u64::try_from(stat.st_dev).map_err(|_| "qualification_cache_payload_file_changed")?,
        stat.st_ino,
    ))
}

fn same_identity(metadata: &fs::Metadata, stat: &Stat) -> bool {
    u64::try_from(stat.st_dev).is_ok_and(|device| metadata.dev() == device)
        && metadata.ino() == stat.st_ino
}

pub(crate) fn same_version(left: &Stat, right: &Stat) -> bool {
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

pub(crate) fn validate_internal_link(relative: &str, target: &str) -> Result<(), &'static str> {
    if target.is_empty()
        || target.len() > MAX_PATH_BYTES
        || target.starts_with('/')
        || target.bytes().any(|byte| byte < 0x20 || byte == 0x7f)
    {
        return Err("qualification_cache_payload_symlink_invalid");
    }
    let parent = relative.rsplit_once('/').map_or("", |(parent, _)| parent);
    let mut depth = parent.split('/').filter(|part| !part.is_empty()).count();
    for component in Path::new(target).components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                depth = depth
                    .checked_sub(1)
                    .ok_or("qualification_cache_payload_symlink_escape")?;
            }
            std::path::Component::Normal(name) => {
                let name = name
                    .to_str()
                    .ok_or("qualification_cache_payload_path_invalid")?;
                if name.is_empty()
                    || name
                        .bytes()
                        .any(|byte| byte < 0x20 || byte == 0x7f || byte == b'/')
                {
                    return Err("qualification_cache_payload_symlink_invalid");
                }
                depth = depth
                    .checked_add(1)
                    .ok_or("qualification_cache_payload_limit")?;
            }
            std::path::Component::RootDir | std::path::Component::Prefix(_) => {
                return Err("qualification_cache_payload_symlink_invalid");
            }
        }
    }
    Ok(())
}

fn update_bytes(hasher: &mut blake3::Hasher, bytes: &[u8]) {
    hasher.update(&(bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

#[path = "payload_copy.rs"]
mod copy;
pub(crate) use copy::copy_tree_to_new_dir;
pub(super) use copy::copy_tree_to_new_dir_owned;
