use std::{
    ffi::{OsStr, OsString},
    fs::{self, File, FileTimes},
    io::{Read, Write},
    os::fd::{AsFd, OwnedFd},
    os::unix::ffi::OsStringExt,
    os::unix::fs::MetadataExt,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use rustix::fs::{self as ufs, Dir, FileType, Mode, OFlags};

use super::{
    READ_CHUNK, SeedLimits,
    execution::SeedError,
    filesystem::{fail, open_parent, openat_dir_os, openat_parts, seed_io_error, statat_os},
};

#[derive(Clone, Copy, Debug)]
pub(super) struct Identity {
    pub(super) dev: u64,
    pub(super) ino: u64,
    pub(super) mode: u32,
    pub(super) size: u64,
    pub(super) atime: (i64, i128),
    pub(super) mtime: (i64, i128),
    pub(super) ctime: (i64, i128),
    pub(super) uid: u32,
    pub(super) gid: u32,
    pub(super) nlink: u64,
}
impl Identity {
    pub(super) fn from_metadata(m: &fs::Metadata) -> Self {
        Self {
            dev: m.dev(),
            ino: m.ino(),
            mode: m.mode(),
            size: m.size(),
            atime: (m.atime(), i128::from(m.atime_nsec())),
            mtime: (m.mtime(), i128::from(m.mtime_nsec())),
            ctime: (m.ctime(), i128::from(m.ctime_nsec())),
            uid: m.uid(),
            gid: m.gid(),
            nlink: m.nlink(),
        }
    }
    pub(super) fn from_stat(s: &rustix::fs::Stat) -> Self {
        Self {
            dev: widen_u64(s.st_dev),
            ino: s.st_ino,
            mode: widen_mode(s.st_mode),
            size: u64::try_from(s.st_size.max(0)).unwrap_or_default(),
            atime: (s.st_atime, i128::from(s.st_atime_nsec)),
            mtime: (s.st_mtime, i128::from(s.st_mtime_nsec)),
            ctime: (s.st_ctime, i128::from(s.st_ctime_nsec)),
            uid: s.st_uid,
            gid: s.st_gid,
            nlink: widen_u64(s.st_nlink),
        }
    }
    pub(super) fn same_object(&self, other: &Self) -> bool {
        self.dev == other.dev && self.ino == other.ino
    }
    pub(super) fn matches_stat(&self, other: &rustix::fs::Stat) -> bool {
        self == &Self::from_stat(other)
    }
}
impl PartialEq for Identity {
    fn eq(&self, other: &Self) -> bool {
        self.dev == other.dev
            && self.ino == other.ino
            && self.mode == other.mode
            && self.size == other.size
            && self.mtime == other.mtime
            && self.ctime == other.ctime
            && self.uid == other.uid
            && self.gid == other.gid
            && self.nlink == other.nlink
    }
}
impl Eq for Identity {}

pub(super) struct Entry {
    pub(super) parts: Vec<OsString>,
    pub(super) identity: Identity,
}
impl Entry {
    pub(super) fn new(parts: Vec<OsString>, stat: rustix::fs::Stat) -> Self {
        Self {
            parts,
            identity: Identity::from_stat(&stat),
        }
    }
}

pub(super) struct Snapshot {
    pub(super) started: Instant,
    pub(super) limits: SeedLimits,
    pub(super) entries: Vec<Entry>,
    pub(super) bytes: u64,
}
impl Snapshot {
    pub(super) fn new(started: Instant, limits: SeedLimits) -> Self {
        Self {
            started,
            limits,
            entries: Vec::new(),
            bytes: 0,
        }
    }
    pub(super) fn checkpoint(&self) -> Result<(), SeedError> {
        if self.started.elapsed() >= self.limits.seconds {
            Err(SeedError::Skip("elapsed limit"))
        } else {
            Ok(())
        }
    }
    pub(super) fn add_bytes(&mut self, bytes: u64) -> Result<(), SeedError> {
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .ok_or_else(|| fail("byte_budget_overflow"))?;
        Ok(())
    }
    pub(super) fn inventory(
        &mut self,
        parent: &OwnedFd,
        prefix: &mut Vec<OsString>,
    ) -> Result<(), SeedError> {
        self.checkpoint()?;
        if prefix.len() > 64 {
            return Err(fail("source_depth_limit"));
        }
        if self.entries.len() >= self.limits.entries {
            return Err(SeedError::Skip("entry limit"));
        }
        let mut names = Vec::new();
        for entry in Dir::read_from(parent).map_err(seed_io_error)? {
            self.checkpoint()?;
            let entry = entry.map_err(seed_io_error)?;
            let name = OsString::from_vec(entry.file_name().to_bytes().to_vec());
            if name.as_os_str() == OsStr::new(".") || name.as_os_str() == OsStr::new("..") {
                continue;
            }
            if is_debug_root(prefix) && name.as_os_str() == OsStr::new(".cargo-lock") {
                continue;
            }
            // Bound enumeration before retaining another filename.
            if self.entries.len().saturating_add(names.len()) >= self.limits.entries {
                return Err(SeedError::Skip("entry limit"));
            }
            names.push(name);
        }
        names.sort();
        for name in names {
            self.checkpoint()?;
            if is_debug_root(prefix) && name.as_os_str() == OsStr::new(".cargo-lock") {
                continue;
            }
            let stat = statat_os(parent, &name).map_err(seed_io_error)?;
            let ty = FileType::from_raw_mode(stat.st_mode);
            if ty != FileType::Directory && ty != FileType::RegularFile {
                return Err(fail("source_symlink_or_special_file"));
            }
            prefix.push(name.clone());
            if self.entries.len() >= self.limits.entries {
                prefix.pop();
                return Err(SeedError::Skip("entry limit"));
            }
            self.entries.push(Entry::new(prefix.clone(), stat));
            if ty == FileType::Directory {
                let child = openat_dir_os(parent, &name).map_err(seed_io_error)?;
                let opened = Identity::from_stat(&ufs::fstat(&child).map_err(seed_io_error)?);
                let entry = self
                    .entries
                    .last()
                    .ok_or_else(|| fail("source_inventory_entry_missing"))?;
                if opened != entry.identity {
                    prefix.pop();
                    return Err(fail("source_directory_changed"));
                }
                self.inventory(&child, prefix)?;
                if Identity::from_stat(&ufs::fstat(&child).map_err(seed_io_error)?) != opened {
                    prefix.pop();
                    return Err(fail("source_directory_changed"));
                }
            } else {
                self.add_bytes(u64::try_from(stat.st_size.max(0)).unwrap_or_default())?;
            }
            prefix.pop();
        }
        Ok(())
    }
    pub(super) fn validate_source(&self, root: &OwnedFd) -> Result<(), SeedError> {
        for entry in &self.entries {
            self.checkpoint()?;
            let (parent, name) = open_parent(root, &entry.parts)?;
            let current = statat_os(&parent, &name).map_err(seed_io_error)?;
            if Identity::from_stat(&current) != entry.identity {
                return Err(fail("source_changed_before_admission"));
            }
        }
        Ok(())
    }
    pub(super) fn copy_to(&self, source: &OwnedFd, stage: &OwnedFd) -> Result<(), SeedError> {
        for entry in &self.entries {
            self.checkpoint()?;
            let (src_parent, name) = open_parent(source, &entry.parts)?;
            if Identity::from_stat(&statat_os(&src_parent, &name).map_err(seed_io_error)?)
                != entry.identity
            {
                return Err(fail("named_source_changed"));
            }
            let dst_parent = openat_parts(stage, &entry.parts[..entry.parts.len() - 1])?;
            let mode = checked_raw_mode::<rustix::fs::RawMode>(entry.identity.mode)?;
            let ty = FileType::from_raw_mode(mode);
            if ty == FileType::Directory {
                ufs::mkdirat(&dst_parent, &name, Mode::from_raw_mode(0o700))
                    .map_err(seed_io_error)?;
            } else {
                let input = open_copy_source(&src_parent, &name, entry.identity)?;
                let output = ufs::openat(
                    &dst_parent,
                    &name,
                    OFlags::WRONLY
                        | OFlags::CREATE
                        | OFlags::EXCL
                        | OFlags::NOFOLLOW
                        | OFlags::CLOEXEC,
                    Mode::from_raw_mode(0o600),
                )
                .map_err(seed_io_error)?;
                copy_file(self, &input, &output, entry.identity)?;
            }
            if Identity::from_stat(&statat_os(&src_parent, &name).map_err(seed_io_error)?)
                != entry.identity
            {
                return Err(fail("source_changed_during_copy"));
            }
        }
        for entry in self.entries.iter().rev() {
            let mode = checked_raw_mode::<rustix::fs::RawMode>(entry.identity.mode)?;
            if FileType::from_raw_mode(mode) != FileType::Directory {
                continue;
            }
            let dir = openat_parts(stage, &entry.parts)?;
            let mode = checked_raw_mode::<rustix::fs::RawMode>(entry.identity.mode & 0o7777)?;
            ufs::fchmod(&dir, Mode::from_raw_mode(mode)).map_err(seed_io_error)?;
            set_times(&dir, &entry.identity)?;
        }
        Ok(())
    }
}

fn copy_file(
    snapshot: &Snapshot,
    input: &OwnedFd,
    output: &OwnedFd,
    expected: Identity,
) -> Result<(), SeedError> {
    let mut input = File::from(input.try_clone().map_err(seed_io_error)?);
    let mut output = File::from(output.try_clone().map_err(seed_io_error)?);
    if Identity::from_metadata(&input.metadata().map_err(seed_io_error)?) != expected {
        return Err(fail("source_file_changed"));
    }
    copy_bounded(snapshot, &mut input, &mut output, expected.size)?;
    if Identity::from_metadata(&input.metadata().map_err(seed_io_error)?) != expected {
        return Err(fail("source_changed_during_copy"));
    }
    let out = output.metadata().map_err(seed_io_error)?;
    if out.nlink() != 1 || (out.dev(), out.ino()) == (expected.dev, expected.ino) {
        return Err(fail("target_copy_not_independent"));
    }
    let mode = checked_raw_mode::<rustix::fs::RawMode>(expected.mode & 0o7777)?;
    ufs::fchmod(&output, Mode::from_raw_mode(mode)).map_err(seed_io_error)?;
    set_times(&output, &expected)?;
    Ok(())
}

fn open_copy_source(
    parent: &OwnedFd,
    name: &OsStr,
    expected: Identity,
) -> Result<OwnedFd, SeedError> {
    let input = ufs::openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(seed_io_error)?;
    if Identity::from_stat(&ufs::fstat(&input).map_err(seed_io_error)?) != expected {
        return Err(fail("source_file_changed"));
    }
    Ok(input)
}

fn is_debug_root(prefix: &[OsString]) -> bool {
    prefix.len() == 1 && prefix[0].as_os_str() == OsStr::new("debug")
}

fn copy_bounded(
    snapshot: &Snapshot,
    input: &mut impl Read,
    output: &mut impl Write,
    expected_size: u64,
) -> Result<(), SeedError> {
    let mut copied = 0u64;
    let mut buffer = vec![0; READ_CHUNK];
    loop {
        snapshot.checkpoint()?;
        let count = input.read(&mut buffer).map_err(seed_io_error)?;
        if count == 0 {
            break;
        }
        copied = copied
            .checked_add(count as u64)
            .ok_or_else(|| fail("byte_budget_overflow"))?;
        if copied > expected_size {
            return Err(fail("source_grew_beyond_budget"));
        }
        output.write_all(&buffer[..count]).map_err(seed_io_error)?;
    }
    if copied != expected_size {
        return Err(fail("source_changed_during_copy"));
    }
    Ok(())
}

fn set_times(fd: &impl AsFd, identity: &Identity) -> Result<(), SeedError> {
    let file = File::from(fd.as_fd().try_clone_to_owned().map_err(seed_io_error)?);
    let accessed = system_time(identity.atime)?;
    let modified = system_time(identity.mtime)?;
    file.set_times(
        FileTimes::new()
            .set_accessed(accessed)
            .set_modified(modified),
    )
    .map_err(seed_io_error)?;
    Ok(())
}

pub(super) fn system_time((seconds, nanos): (i64, i128)) -> Result<SystemTime, SeedError> {
    const NANOS_PER_SECOND: i128 = 1_000_000_000;
    if !(0..NANOS_PER_SECOND).contains(&nanos) {
        return Err(fail("source_timestamp_invalid"));
    }
    let nanoseconds = i128::from(seconds) * NANOS_PER_SECOND + nanos;
    let magnitude = nanoseconds.unsigned_abs();
    let seconds = u64::try_from(magnitude / 1_000_000_000)
        .map_err(|_| fail("source_timestamp_out_of_range"))?;
    let nanos = u32::try_from(magnitude % 1_000_000_000)
        .map_err(|_| fail("source_timestamp_out_of_range"))?;
    let duration = Duration::new(seconds, nanos);
    if nanoseconds < 0 {
        UNIX_EPOCH
            .checked_sub(duration)
            .ok_or_else(|| fail("source_timestamp_out_of_range"))
    } else {
        UNIX_EPOCH
            .checked_add(duration)
            .ok_or_else(|| fail("source_timestamp_out_of_range"))
    }
}

fn widen_mode<Mode>(mode: Mode) -> u32
where
    Mode: Into<u32>,
{
    mode.into()
}

fn widen_u64<Value>(value: Value) -> u64
where
    Value: TryInto<u64>,
{
    value.try_into().unwrap_or_default()
}

pub(super) fn checked_raw_mode<Mode>(mode: u32) -> Result<Mode, SeedError>
where
    Mode: TryFrom<u32>,
{
    mode.try_into()
        .map_err(|_| fail("source_file_mode_invalid"))
}

#[cfg(test)]
#[path = "parallel_target_seed_inventory_tests.rs"]
mod tests;
