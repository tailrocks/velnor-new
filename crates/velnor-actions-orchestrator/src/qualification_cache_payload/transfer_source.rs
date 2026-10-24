use super::dirs::{c_name, same_object, same_version, validate_file_stat};
use rustix::fs::{AtFlags, FileType, Mode, OFlags, Stat, open, openat, statat};
use std::ffi::CString;
use std::fs;
use std::os::unix::ffi::OsStrExt;
#[cfg(target_os = "linux")]
use std::path::PathBuf;
use std::path::{Component, Path};

const MAX_SOURCE_PATH_BYTES: usize = 4_096;
const MAX_SOURCE_COMPONENTS: usize = 128;

struct Edge {
    name: CString,
    child: rustix::fd::OwnedFd,
    opened: Stat,
}

/// Holds every directory descriptor used to reach one cache source root.
pub(in crate::qualification_cache_payload) struct SourceDirectory {
    root: rustix::fd::OwnedFd,
    edges: Vec<Edge>,
    #[cfg(target_os = "linux")]
    target: PathBuf,
}

impl SourceDirectory {
    pub(in crate::qualification_cache_payload) fn open(path: &Path) -> Result<Self, &'static str> {
        Self::open_optional(path)?.ok_or("qualification_cache_payload_missing")
    }

    pub(in crate::qualification_cache_payload) fn open_optional(
        path: &Path,
    ) -> Result<Option<Self>, &'static str> {
        let names = components(path)?;
        let root = open(
            "/",
            OFlags::RDONLY
                | OFlags::DIRECTORY
                | OFlags::NOFOLLOW
                | OFlags::NONBLOCK
                | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| "qualification_cache_payload_open_failed")?;
        let root_stat =
            rustix::fs::fstat(&root).map_err(|_| "qualification_cache_payload_open_failed")?;
        validate_ancestor(&root_stat)?;
        let mut result = Self {
            root,
            edges: Vec::with_capacity(names.len()),
            #[cfg(target_os = "linux")]
            target: path.to_path_buf(),
        };
        for (index, name) in names.iter().enumerate() {
            let parent = result.current_fd();
            let Some(before) = optional_entry(parent, name)? else {
                result.confirm_absent(name.as_c_str(), false)?;
                return Ok(None);
            };
            let final_component = index + 1 == names.len();
            validate_component(&before, final_component)?;
            let child = openat(
                parent,
                name.as_c_str(),
                OFlags::RDONLY
                    | OFlags::DIRECTORY
                    | OFlags::NOFOLLOW
                    | OFlags::NONBLOCK
                    | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|_| "qualification_cache_payload_root_invalid")?;
            let opened =
                rustix::fs::fstat(&child).map_err(|_| "qualification_cache_payload_open_failed")?;
            validate_component(&opened, final_component)?;
            if !same_security_object(&before, &opened) {
                return Err("qualification_cache_payload_file_changed");
            }
            result.edges.push(Edge {
                name: name.clone(),
                child,
                opened,
            });
        }
        if result.edges.is_empty() {
            return Err("qualification_cache_payload_root_invalid");
        }
        result.revalidate()?;
        Ok(Some(result))
    }

    pub(in crate::qualification_cache_payload) fn fd(&self) -> &rustix::fd::OwnedFd {
        self.edges.last().map_or(&self.root, |edge| &edge.child)
    }

    #[cfg(target_os = "linux")]
    pub(in crate::qualification_cache_payload) fn target(&self) -> &Path {
        &self.target
    }

    pub(in crate::qualification_cache_payload) fn revalidate(&self) -> Result<(), &'static str> {
        self.revalidate_path(true)
    }

    fn revalidate_path(&self, final_edge_is_root: bool) -> Result<(), &'static str> {
        let root_stat =
            rustix::fs::fstat(&self.root).map_err(|_| "qualification_cache_payload_open_failed")?;
        validate_ancestor(&root_stat)?;
        for (index, edge) in self.edges.iter().enumerate() {
            let parent = self
                .edges
                .get(index.wrapping_sub(1))
                .map_or(&self.root, |previous| &previous.child);
            let parent_stat =
                rustix::fs::fstat(parent).map_err(|_| "qualification_cache_payload_open_failed")?;
            validate_ancestor(&parent_stat)?;
            let final_component = final_edge_is_root && index + 1 == self.edges.len();
            let opened = rustix::fs::fstat(&edge.child)
                .map_err(|_| "qualification_cache_payload_open_failed")?;
            let current = statat(parent, edge.name.as_c_str(), AtFlags::SYMLINK_NOFOLLOW)
                .map_err(|_| "qualification_cache_payload_file_changed")?;
            validate_component(&opened, final_component)?;
            validate_component(&current, final_component)?;
            if !same_security_object(&edge.opened, &opened)
                || !same_security_object(&opened, &current)
            {
                return Err("qualification_cache_payload_file_changed");
            }
        }
        Ok(())
    }

    fn confirm_absent(
        &self,
        name: &std::ffi::CStr,
        parent_is_complete_path: bool,
    ) -> Result<(), &'static str> {
        self.revalidate_path(parent_is_complete_path)?;
        match statat(self.fd(), name, AtFlags::SYMLINK_NOFOLLOW) {
            Err(error) if error == rustix::io::Errno::NOENT => Ok(()),
            Ok(_) => Err("qualification_cache_payload_file_changed"),
            Err(_) => Err("qualification_cache_payload_open_failed"),
        }
    }

    fn current_fd(&self) -> &rustix::fd::OwnedFd {
        self.fd()
    }
}

pub(in crate::qualification_cache_payload) struct SourceFileGuard {
    parent: SourceDirectory,
    name: CString,
    opened: Stat,
}

impl SourceFileGuard {
    pub(in crate::qualification_cache_payload) fn revalidate(
        &self,
        file: &fs::File,
    ) -> Result<(), &'static str> {
        self.parent.revalidate()?;
        let descriptor =
            rustix::fs::fstat(file).map_err(|_| "qualification_cache_payload_read_failed")?;
        let current = statat(
            self.parent.fd(),
            self.name.as_c_str(),
            AtFlags::SYMLINK_NOFOLLOW,
        )
        .map_err(|_| "qualification_cache_payload_file_changed")?;
        validate_file_stat(&descriptor)?;
        if !same_version(&self.opened, &descriptor) || !same_version(&self.opened, &current) {
            return Err("qualification_cache_payload_file_changed");
        }
        Ok(())
    }
}

pub(super) fn open_file(path: &Path) -> Result<(fs::File, Stat, SourceFileGuard), &'static str> {
    open_optional_file(path)?.ok_or("qualification_cache_payload_open_failed")
}

pub(super) fn open_optional_file(
    path: &Path,
) -> Result<Option<(fs::File, Stat, SourceFileGuard)>, &'static str> {
    let parent_path = path
        .parent()
        .ok_or("qualification_cache_payload_path_invalid")?;
    let name = c_name(
        path.file_name()
            .ok_or("qualification_cache_payload_path_invalid")?,
    )?;
    let Some(parent) = SourceDirectory::open_optional(parent_path)? else {
        return Ok(None);
    };
    let Some(before) = optional_entry(parent.fd(), &name)? else {
        parent.confirm_absent(name.as_c_str(), true)?;
        return Ok(None);
    };
    if FileType::from_raw_mode(before.st_mode) != FileType::RegularFile {
        return Err("qualification_cache_root_kind_mismatch");
    }
    validate_file_stat(&before)?;
    let descriptor = openat(
        parent.fd(),
        name.as_c_str(),
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| "qualification_cache_payload_open_failed")?;
    let opened =
        rustix::fs::fstat(&descriptor).map_err(|_| "qualification_cache_payload_open_failed")?;
    if !same_version(&before, &opened) {
        return Err("qualification_cache_payload_file_changed");
    }
    let guard = SourceFileGuard {
        parent,
        name,
        opened,
    };
    let file = fs::File::from(descriptor);
    guard.revalidate(&file)?;
    Ok(Some((file, opened, guard)))
}

fn components(path: &Path) -> Result<Vec<CString>, &'static str> {
    if !path.is_absolute() || path.as_os_str().as_bytes().len() > MAX_SOURCE_PATH_BYTES {
        return Err("qualification_cache_payload_path_invalid");
    }
    let mut names = Vec::new();
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => {
                if names.len() == MAX_SOURCE_COMPONENTS {
                    return Err("qualification_cache_payload_path_invalid");
                }
                names.push(c_name(name)?);
            }
            Component::CurDir | Component::ParentDir | Component::Prefix(_) => {
                return Err("qualification_cache_payload_path_invalid");
            }
        }
    }
    if names.is_empty() {
        return Err("qualification_cache_payload_root_invalid");
    }
    Ok(names)
}

fn optional_entry(
    parent: &rustix::fd::OwnedFd,
    name: &std::ffi::CStr,
) -> Result<Option<Stat>, &'static str> {
    match statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) => Ok(Some(stat)),
        Err(error) if error == rustix::io::Errno::NOENT => Ok(None),
        Err(_) => Err("qualification_cache_payload_open_failed"),
    }
}

fn validate_component(stat: &Stat, final_component: bool) -> Result<(), &'static str> {
    if FileType::from_raw_mode(stat.st_mode) != FileType::Directory {
        return Err("qualification_cache_payload_root_invalid");
    }
    if final_component {
        if stat.st_uid != crate::filesystem_owner::effective_uid() {
            return Err("qualification_cache_payload_owner_or_mode_invalid");
        }
        super::payload::validate_payload_root_mode(stat)
    } else {
        validate_ancestor(stat)
    }
}

fn validate_ancestor(stat: &Stat) -> Result<(), &'static str> {
    let owner = crate::filesystem_owner::effective_uid();
    let root_owned_sticky = stat.st_uid == 0 && stat.st_mode & 0o1000 != 0;
    if FileType::from_raw_mode(stat.st_mode) != FileType::Directory
        || (stat.st_uid != 0 && stat.st_uid != owner)
        || stat.st_mode & 0o6000 != 0
        || (stat.st_mode & 0o1000 != 0 && !root_owned_sticky)
        || (stat.st_mode & 0o022 != 0 && !root_owned_sticky)
    {
        return Err("qualification_cache_payload_source_ancestor_invalid");
    }
    Ok(())
}

fn same_security_object(left: &Stat, right: &Stat) -> bool {
    same_object(left, right)
        && left.st_uid == right.st_uid
        && left.st_mode == right.st_mode
        && FileType::from_raw_mode(left.st_mode) == FileType::Directory
}

#[cfg(test)]
#[path = "tests/dispatch_cache_source_authority_tests.rs"]
mod tests;
