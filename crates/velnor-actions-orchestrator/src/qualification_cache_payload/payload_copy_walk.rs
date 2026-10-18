//! Walk payload entries and copy each supported type.

use std::collections::BTreeMap;
use std::ffi::CString;
use std::fs;
use std::io::{Read, copy};
use std::path::Path;

use rustix::fs::{AtFlags, Dir, FileType, Mode, OFlags, Stat, fchmod, openat, statat};

use super::super::{MAX_DEPTH, MAX_ENTRIES, MAX_PATH_BYTES, MAX_TOTAL_BYTES};
use super::open_directory_at;

pub(super) struct CopyContext<'a, F> {
    destination_root_fd: &'a rustix::fd::OwnedFd,
    hardlinks: BTreeMap<(u64, u64), String>,
    budget: CopyBudget,
    after_source_open: &'a mut F,
}

impl<'a, F> CopyContext<'a, F> {
    pub(super) fn new(
        destination_root_fd: &'a rustix::fd::OwnedFd,
        after_source_open: &'a mut F,
    ) -> Self {
        Self {
            destination_root_fd,
            hardlinks: BTreeMap::new(),
            budget: CopyBudget::default(),
            after_source_open,
        }
    }
}

pub(super) fn copy_directory<F>(
    source_fd: &rustix::fd::OwnedFd,
    destination_fd: &rustix::fd::OwnedFd,
    depth: usize,
    relative: &str,
    context: &mut CopyContext<'_, F>,
) -> Result<(), &'static str>
where
    F: FnMut(&Path),
{
    if depth > MAX_DEPTH {
        return Err("qualification_cache_payload_limit");
    }
    let start_stat = fstat(source_fd)?;
    for name in child_names(source_fd)? {
        copy_entry(source_fd, destination_fd, &name, relative, depth, context)?;
    }
    if !super::super::same_version(&start_stat, &fstat(source_fd)?) {
        return Err("qualification_cache_payload_file_changed");
    }
    Ok(())
}

fn child_names(directory_fd: &rustix::fd::OwnedFd) -> Result<Vec<CString>, &'static str> {
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

fn copy_entry<F>(
    source_fd: &rustix::fd::OwnedFd,
    destination_fd: &rustix::fd::OwnedFd,
    name: &CString,
    parent: &str,
    depth: usize,
    context: &mut CopyContext<'_, F>,
) -> Result<(), &'static str>
where
    F: FnMut(&Path),
{
    let text = name
        .to_str()
        .map_err(|_| "qualification_cache_payload_path_invalid")?;
    if text == "." || text == ".." {
        return Ok(());
    }
    if text.is_empty()
        || text
            .bytes()
            .any(|byte| byte < 0x20 || byte == 0x7f || byte == b'/')
    {
        return Err("qualification_cache_payload_path_invalid");
    }
    let relative = if parent.is_empty() {
        text.to_owned()
    } else {
        format!("{parent}/{text}")
    };
    let metadata = statat(source_fd, name.as_c_str(), AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|_| "qualification_cache_payload_read_failed")?;
    super::super::validate_entry_owner_mode(&metadata)?;
    let kind = FileType::from_raw_mode(metadata.st_mode);
    let size = match kind {
        FileType::RegularFile => file_size(&metadata)?,
        FileType::Directory | FileType::Symlink => 0,
        _ => return Err("qualification_cache_payload_entry_unsupported"),
    };
    context.budget.add_entry(&relative, size)?;
    match kind {
        FileType::Directory => copy_subdirectory(
            source_fd,
            destination_fd,
            name,
            &relative,
            depth,
            &metadata,
            context,
        ),
        FileType::RegularFile => copy_file(
            source_fd,
            destination_fd,
            name,
            &relative,
            &metadata,
            context,
        ),
        FileType::Symlink => copy_symlink(
            source_fd,
            destination_fd,
            name,
            &relative,
            &metadata,
            &mut context.budget,
        ),
        _ => Err("qualification_cache_payload_entry_unsupported"),
    }
}

fn copy_subdirectory<F>(
    source_parent: &rustix::fd::OwnedFd,
    destination_parent: &rustix::fd::OwnedFd,
    name: &std::ffi::CStr,
    relative: &str,
    depth: usize,
    metadata: &Stat,
    context: &mut CopyContext<'_, F>,
) -> Result<(), &'static str>
where
    F: FnMut(&Path),
{
    rustix::fs::mkdirat(
        destination_parent,
        name,
        Mode::RUSR | Mode::WUSR | Mode::XUSR,
    )
    .map_err(|_| "qualification_cache_payload_write_failed")?;
    let source_child = open_directory_at(source_parent, name)?;
    if !super::super::same_version(metadata, &fstat(&source_child)?) {
        return Err("qualification_cache_payload_file_changed");
    }
    let destination_child = open_directory_at(destination_parent, name)?;
    copy_directory(
        &source_child,
        &destination_child,
        depth + 1,
        relative,
        context,
    )?;
    fchmod(
        &destination_child,
        Mode::from_raw_mode(metadata.st_mode & 0o777),
    )
    .map_err(|_| "qualification_cache_payload_write_failed")?;
    if !super::super::same_version(metadata, &fstat(&source_child)?) {
        return Err("qualification_cache_payload_file_changed");
    }
    Ok(())
}

fn copy_file<F>(
    source_parent: &rustix::fd::OwnedFd,
    destination_parent: &rustix::fd::OwnedFd,
    name: &std::ffi::CStr,
    relative: &str,
    metadata: &Stat,
    context: &mut CopyContext<'_, F>,
) -> Result<(), &'static str>
where
    F: FnMut(&Path),
{
    let source = openat(
        source_parent,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| "qualification_cache_payload_open_failed")?;
    let source_stat = fstat(&source)?;
    if FileType::from_raw_mode(source_stat.st_mode) != FileType::RegularFile
        || !super::super::same_version(metadata, &source_stat)
    {
        return Err("qualification_cache_payload_file_changed");
    }
    (context.after_source_open)(Path::new(relative));
    let identity = super::super::file_identity(metadata)?;
    if let Some(first) = context.hardlinks.get(&identity) {
        rustix::fs::linkat(
            context.destination_root_fd,
            first.as_str(),
            destination_parent,
            name,
            AtFlags::empty(),
        )
        .map_err(|_| "qualification_cache_payload_write_failed")?;
    } else {
        copy_new_file(
            &source,
            destination_parent,
            name,
            metadata,
            identity,
            relative,
            &mut context.hardlinks,
        )?;
    }
    if !super::super::same_version(metadata, &fstat(&source)?)
        || !super::super::same_version(
            metadata,
            &statat(source_parent, name, AtFlags::SYMLINK_NOFOLLOW)
                .map_err(|_| "qualification_cache_payload_read_failed")?,
        )
    {
        return Err("qualification_cache_payload_file_changed");
    }
    Ok(())
}

fn copy_new_file(
    source: &rustix::fd::OwnedFd,
    destination_parent: &rustix::fd::OwnedFd,
    name: &std::ffi::CStr,
    metadata: &Stat,
    identity: (u64, u64),
    relative: &str,
    hardlinks: &mut BTreeMap<(u64, u64), String>,
) -> Result<(), &'static str> {
    let destination = openat(
        destination_parent,
        name,
        OFlags::WRONLY
            | OFlags::CREATE
            | OFlags::EXCL
            | OFlags::NOFOLLOW
            | OFlags::NONBLOCK
            | OFlags::CLOEXEC,
        Mode::from_raw_mode(metadata.st_mode & 0o777),
    )
    .map_err(|_| "qualification_cache_payload_write_failed")?;
    let source_copy =
        rustix::io::dup(source).map_err(|_| "qualification_cache_payload_open_failed")?;
    let mut source_file = fs::File::from(source_copy);
    let mut destination_file = fs::File::from(destination);
    let size = file_size(metadata)?;
    let mut bounded = (&mut source_file).take(size);
    let copied = copy(&mut bounded, &mut destination_file)
        .map_err(|_| "qualification_cache_payload_write_failed")?;
    if copied != size {
        return Err("qualification_cache_payload_file_changed");
    }
    fchmod(
        &destination_file,
        Mode::from_raw_mode(metadata.st_mode & 0o777),
    )
    .map_err(|_| "qualification_cache_payload_write_failed")?;
    let source_end =
        rustix::fs::fstat(&source_file).map_err(|_| "qualification_cache_payload_read_failed")?;
    if !super::super::same_version(metadata, &source_end) {
        return Err("qualification_cache_payload_file_changed");
    }
    hardlinks.insert(identity, relative.to_owned());
    Ok(())
}

fn copy_symlink(
    source_parent: &rustix::fd::OwnedFd,
    destination_parent: &rustix::fd::OwnedFd,
    name: &std::ffi::CStr,
    relative: &str,
    metadata: &Stat,
    budget: &mut CopyBudget,
) -> Result<(), &'static str> {
    if metadata.st_nlink != 1 {
        return Err("qualification_cache_payload_entry_unsupported");
    }
    let target = rustix::fs::readlinkat(source_parent, name, Vec::new())
        .map_err(|_| "qualification_cache_payload_read_failed")?
        .into_string()
        .map_err(|_| "qualification_cache_payload_path_invalid")?;
    super::super::validate_internal_link(relative, &target)?;
    budget
        .add_bytes(u64::try_from(target.len()).map_err(|_| "qualification_cache_payload_limit")?)?;
    rustix::fs::symlinkat(&target, destination_parent, name)
        .map_err(|_| "qualification_cache_payload_write_failed")?;
    let after = statat(source_parent, name, AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|_| "qualification_cache_payload_read_failed")?;
    if !super::super::same_version(metadata, &after)
        || target.len() != usize::try_from(metadata.st_size).unwrap_or_default()
    {
        return Err("qualification_cache_payload_file_changed");
    }
    Ok(())
}

fn fstat(fd: &rustix::fd::OwnedFd) -> Result<Stat, &'static str> {
    rustix::fs::fstat(fd).map_err(|_| "qualification_cache_payload_open_failed")
}

fn file_size(metadata: &Stat) -> Result<u64, &'static str> {
    u64::try_from(metadata.st_size).map_err(|_| "qualification_cache_payload_limit")
}

#[derive(Default)]
struct CopyBudget {
    entries: usize,
    bytes: u64,
}

impl CopyBudget {
    fn add_entry(&mut self, path: &str, bytes: u64) -> Result<(), &'static str> {
        if path.len() > MAX_PATH_BYTES || self.entries >= MAX_ENTRIES {
            return Err("qualification_cache_payload_limit");
        }
        self.entries += 1;
        self.add_bytes(bytes)
    }

    fn add_bytes(&mut self, bytes: u64) -> Result<(), &'static str> {
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .filter(|total| *total <= MAX_TOTAL_BYTES)
            .ok_or("qualification_cache_payload_limit")?;
        Ok(())
    }
}
