use std::fs;
use std::path::Path;

use rustix::fs::{AtFlags, Mode, OFlags, Stat, openat, statat};

use super::{
    DOMAIN, Tree, payload_format_version, same_version, update_bytes, validate_directory,
    validate_payload_root_mode,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArchiveEntryKind {
    File,
    Directory,
    Symlink,
}

impl ArchiveEntryKind {
    pub(crate) const fn tag(self) -> u8 {
        match self {
            Self::File => b'f',
            Self::Directory => b'd',
            Self::Symlink => b'l',
        }
    }
}

/// One checked archive member from the source tree or a parsed transport.
#[derive(Debug, Clone)]
pub(crate) struct ArchiveEntry {
    pub(crate) path: String,
    pub(crate) kind: ArchiveEntryKind,
    pub(crate) mode: u32,
    pub(crate) size: u64,
    pub(crate) content_digest: [u8; 32],
    pub(crate) hardlink_target: Option<String>,
    pub(crate) link_target: Option<String>,
    pub(crate) stat: Option<Stat>,
}

#[derive(Debug)]
pub(crate) struct ArchiveInventory {
    pub(crate) root_mode: u32,
    pub(crate) digest: String,
    pub(crate) entries: Vec<ArchiveEntry>,
    pub(crate) root_stat: Stat,
}

pub(super) fn inventory_from_tree(
    root_stat: Stat,
    mut tree: Tree,
) -> Result<ArchiveInventory, &'static str> {
    if tree
        .file_links
        .values()
        .any(|(expected, observed)| expected != observed)
    {
        return Err("qualification_cache_payload_hardlink_escape");
    }
    tree.entries
        .sort_by(|left, right| left.path.cmp(&right.path));
    let mut first_paths = std::collections::BTreeMap::new();
    for entry in &tree.entries {
        if let Some(identity) = entry.file_identity {
            first_paths
                .entry(identity)
                .or_insert_with(|| entry.path.clone());
        }
    }
    let entries = tree
        .entries
        .into_iter()
        .map(|entry| {
            let hardlink_target = entry.file_identity.and_then(|identity| {
                tree.file_links
                    .get(&identity)
                    .filter(|(_, count)| *count > 1)
                    .and_then(|_| first_paths.get(&identity))
                    .filter(|first| first.as_str() != entry.path)
                    .cloned()
            });
            ArchiveEntry {
                path: entry.path,
                kind: match entry.kind {
                    b'f' => ArchiveEntryKind::File,
                    b'd' => ArchiveEntryKind::Directory,
                    b'l' => ArchiveEntryKind::Symlink,
                    _ => unreachable!("payload scanner emits closed entry kinds"),
                },
                mode: entry.mode,
                size: entry.size,
                content_digest: entry.content_digest,
                hardlink_target,
                link_target: entry.link_target,
                stat: Some(entry.stat),
            }
        })
        .collect::<Vec<_>>();
    let root_mode = u32::from(root_stat.st_mode) & 0o777;
    let digest = digest_archive_entries(root_mode, &entries)?;
    Ok(ArchiveInventory {
        root_mode,
        digest,
        entries,
        root_stat,
    })
}

/// Give the existing useful-state digest for checked archive entries.
pub(crate) fn digest_archive_entries(
    root_mode: u32,
    entries: &[ArchiveEntry],
) -> Result<String, &'static str> {
    if entries.windows(2).any(|pair| pair[0].path >= pair[1].path) {
        return Err("qualification_cache_payload_path_order_invalid");
    }
    let mut digest = blake3::Hasher::new();
    digest.update(DOMAIN);
    digest.update(&payload_format_version().to_be_bytes());
    digest.update(&root_mode.to_be_bytes());
    for entry in entries {
        update_bytes(&mut digest, entry.path.as_bytes());
        digest.update(&[entry.kind.tag()]);
        digest.update(&entry.mode.to_be_bytes());
        digest.update(&entry.size.to_be_bytes());
        digest.update(&entry.content_digest);
        if entry.kind == ArchiveEntryKind::File {
            if let Some(first) = &entry.hardlink_target {
                digest.update(b"h");
                update_bytes(&mut digest, first.as_bytes());
            } else {
                digest.update(b"f");
            }
        } else {
            digest.update(&[0]);
        }
    }
    Ok(format!("b3-{}", digest.finalize().to_hex()))
}

pub(crate) fn inventory_directory(
    root: &Path,
) -> Result<Option<(ArchiveInventory, super::super::source::SourceDirectory)>, &'static str> {
    let source = super::super::source::SourceDirectory::open(root)?;
    let root_stat =
        rustix::fs::fstat(source.fd()).map_err(|_| "qualification_cache_payload_open_failed")?;
    validate_directory(&root_stat)?;
    validate_payload_root_mode(&root_stat)?;
    let mut tree = Tree::default();
    super::scan::collect(source.fd(), "", 0, &mut tree)?;
    let end_stat =
        rustix::fs::fstat(source.fd()).map_err(|_| "qualification_cache_payload_open_failed")?;
    if !same_version(&root_stat, &end_stat) {
        return Err("qualification_cache_payload_file_changed");
    }
    source.revalidate()?;
    if tree.entries.is_empty() {
        return Ok(None);
    }
    inventory_from_tree(root_stat, tree).map(|inventory| Some((inventory, source)))
}

pub(crate) fn open_inventory_file(
    root_fd: &rustix::fd::OwnedFd,
    entry: &ArchiveEntry,
) -> Result<fs::File, &'static str> {
    if entry.kind != ArchiveEntryKind::File {
        return Err("qualification_cache_payload_entry_unsupported");
    }
    let expected = entry
        .stat
        .as_ref()
        .ok_or("qualification_cache_payload_file_changed")?;
    let (parent, name) = open_inventory_parent(root_fd, &entry.path)?;
    let descriptor = openat(
        &parent,
        name.as_c_str(),
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| "qualification_cache_payload_open_failed")?;
    let opened =
        rustix::fs::fstat(&descriptor).map_err(|_| "qualification_cache_payload_open_failed")?;
    if !same_version(expected, &opened) {
        return Err("qualification_cache_payload_file_changed");
    }
    Ok(fs::File::from(descriptor))
}

pub(crate) fn validate_inventory_symlink(
    root_fd: &rustix::fd::OwnedFd,
    entry: &ArchiveEntry,
) -> Result<String, &'static str> {
    if entry.kind != ArchiveEntryKind::Symlink {
        return Err("qualification_cache_payload_entry_unsupported");
    }
    let expected = entry
        .stat
        .as_ref()
        .ok_or("qualification_cache_payload_file_changed")?;
    let expected_target = entry
        .link_target
        .as_deref()
        .ok_or("qualification_cache_payload_file_changed")?;
    let (parent, name) = open_inventory_parent(root_fd, &entry.path)?;
    let current = statat(&parent, name.as_c_str(), AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|_| "qualification_cache_payload_file_changed")?;
    if !same_version(expected, &current) {
        return Err("qualification_cache_payload_file_changed");
    }
    let target = rustix::fs::readlinkat(&parent, name.as_c_str(), Vec::new())
        .map_err(|_| "qualification_cache_payload_read_failed")?
        .into_string()
        .map_err(|_| "qualification_cache_payload_path_invalid")?;
    if target != expected_target {
        return Err("qualification_cache_payload_file_changed");
    }
    Ok(target)
}

pub(crate) fn validate_inventory_directory(
    root_fd: &rustix::fd::OwnedFd,
    entry: &ArchiveEntry,
) -> Result<(), &'static str> {
    if entry.kind != ArchiveEntryKind::Directory {
        return Err("qualification_cache_payload_entry_unsupported");
    }
    let expected = entry
        .stat
        .as_ref()
        .ok_or("qualification_cache_payload_file_changed")?;
    let (parent, name) = open_inventory_parent(root_fd, &entry.path)?;
    let descriptor = openat(
        &parent,
        name.as_c_str(),
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| "qualification_cache_payload_open_failed")?;
    let actual =
        rustix::fs::fstat(&descriptor).map_err(|_| "qualification_cache_payload_read_failed")?;
    if same_version(expected, &actual) {
        Ok(())
    } else {
        Err("qualification_cache_payload_file_changed")
    }
}

fn open_inventory_parent(
    root_fd: &rustix::fd::OwnedFd,
    path: &str,
) -> Result<(rustix::fd::OwnedFd, std::ffi::CString), &'static str> {
    let mut components = path.split('/').peekable();
    let mut parent =
        rustix::io::dup(root_fd).map_err(|_| "qualification_cache_payload_open_failed")?;
    while let Some(component) = components.next() {
        if component.is_empty() || matches!(component, "." | "..") {
            return Err("qualification_cache_payload_path_invalid");
        }
        let name = std::ffi::CString::new(component)
            .map_err(|_| "qualification_cache_payload_path_invalid")?;
        if components.peek().is_none() {
            return Ok((parent, name));
        }
        parent = openat(
            &parent,
            name.as_c_str(),
            OFlags::RDONLY
                | OFlags::DIRECTORY
                | OFlags::NOFOLLOW
                | OFlags::NONBLOCK
                | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| "qualification_cache_payload_open_failed")?;
    }
    Err("qualification_cache_payload_path_invalid")
}
