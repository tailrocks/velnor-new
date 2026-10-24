//! Exact, bounded extraction for immutable release-candidate TAR files.

use std::ffi::CString;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path};
use std::sync::atomic::{AtomicU64, Ordering};

use rustix::fs::{self as unix_fs, AtFlags, Mode, OFlags};

const MEMBER_LIMIT: u64 = 256 * 1024 * 1024;
const TRAILER_LIMIT: usize = 16 * 1024;
const BLOCK: usize = 512;
static TEMP_ID: AtomicU64 = AtomicU64::new(0);

struct Member {
    name: String,
    data_start: usize,
    size: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileIdentity {
    device: i128,
    inode: i128,
    size: i128,
    mtime_seconds: i128,
    mtime_nanoseconds: i128,
    ctime_seconds: i128,
    ctime_nanoseconds: i128,
    mode: i128,
    links: i128,
}

impl FileIdentity {
    fn same_snapshot(self, other: Self) -> bool {
        self == other
    }

    fn same_object(self, other: Self) -> bool {
        self.device == other.device
            && self.inode == other.inode
            && self.size == other.size
            && self.mtime_seconds == other.mtime_seconds
            && self.mtime_nanoseconds == other.mtime_nanoseconds
            && self.mode == other.mode
    }

    fn same_inode(self, other: Self) -> bool {
        self.device == other.device && self.inode == other.inode
    }
}

struct Temporary {
    name: CString,
    file: File,
    identity: FileIdentity,
}

struct Published {
    name: CString,
    identity: FileIdentity,
}

pub(super) fn extract(
    archive: &Path,
    destination: &Path,
    expected: [&str; 3],
) -> Result<(), String> {
    validate_names(expected)?;
    let directory = open_private_directory(destination)?;
    let (archive_parent, archive_name) = open_trusted_parent(archive)?;
    let mut archive = open_archive(&archive_parent, &archive_name)?;
    let before = unix_fs::fstat(&archive)
        .map_err(|error| format!("candidate archive metadata unavailable: {error}"))?;
    if unix_fs::FileType::from_raw_mode(before.st_mode) != unix_fs::FileType::RegularFile {
        return Err("candidate archive is not a regular file".to_owned());
    }
    let length = usize::try_from(before.st_size)
        .map_err(|_| "candidate archive size conversion failed".to_owned())?;
    let total_limit = usize::try_from(MEMBER_LIMIT)
        .ok()
        .and_then(|limit| limit.checked_add(TRAILER_LIMIT))
        .ok_or_else(|| "candidate archive size limit overflow".to_owned())?;
    if length > total_limit || length % BLOCK != 0 {
        return Err("candidate archive exceeds size limit or is truncated".to_owned());
    }

    let mut bytes = Vec::with_capacity(length);
    (&mut archive)
        .take(
            u64::try_from(total_limit + 1)
                .map_err(|_| "candidate read limit overflow".to_owned())?,
        )
        .read_to_end(&mut bytes)
        .map_err(|error| format!("candidate archive read failed: {error}"))?;
    let after = unix_fs::fstat(&archive)
        .map_err(|error| format!("candidate archive metadata unavailable: {error}"))?;
    let named = stat_at(&archive_parent, &archive_name)
        .map_err(|error| format!("candidate archive path metadata unavailable: {error}"))?;
    if !file_identity(&before).same_snapshot(file_identity(&after))
        || (before.st_dev, before.st_ino) != (named.st_dev, named.st_ino)
    {
        return Err("candidate archive changed while reading".to_owned());
    }
    if bytes.len() != length || bytes.len() > total_limit || bytes.len() % BLOCK != 0 {
        return Err("candidate archive changed while reading".to_owned());
    }
    super::validate(bytes.as_slice(), "candidate")?;

    let members = parse_candidate(&bytes, expected)?;
    let mut temporary = Vec::<Temporary>::new();
    let preparation = (|| {
        for (index, member) in members.iter().enumerate() {
            temporary.push(create_temp_at(&directory)?);
            let entry = temporary
                .last_mut()
                .ok_or_else(|| "candidate temporary-file state is missing".to_owned())?;
            let end = member
                .data_start
                .checked_add(member.size)
                .ok_or_else(|| "candidate member bounds overflow".to_owned())?;
            entry
                .file
                .write_all(&bytes[member.data_start..end])
                .map_err(|error| format!("candidate member write failed: {error}"))?;
            let mode = if index == 0 { 0o755 } else { 0o644 };
            unix_fs::fchmod(&entry.file, Mode::from_raw_mode(mode))
                .map_err(|error| format!("candidate member mode update failed: {error}"))?;
            entry
                .file
                .sync_all()
                .map_err(|error| format!("candidate member sync failed: {error}"))?;
            entry.identity = identity_from_fd(&entry.file)?;
        }
        Ok(())
    })();
    if let Err(error) = preparation {
        cleanup_temporaries(&directory, &temporary);
        return Err(error);
    }
    publish_files(&directory, expected, &temporary, |_| Ok(()))
}

fn publish_files<F>(
    directory: &File,
    expected: [&str; 3],
    temporary: &[Temporary],
    mut after_publish: F,
) -> Result<(), String>
where
    F: FnMut(usize) -> Result<(), String>,
{
    let mut published = Vec::<Published>::new();
    let result = (|| {
        for name in expected {
            if exists_at(directory, name)? {
                return Err("candidate output already exists".to_owned());
            }
        }
        for (index, (entry, name)) in temporary.iter().zip(expected).enumerate() {
            let held_identity = identity_from_fd(&entry.file)?;
            if !held_identity.same_snapshot(entry.identity)
                || !named_file_matches(directory, &entry.name, entry.identity, 1, false, true)
            {
                return Err("candidate temporary file changed before publication".to_owned());
            }
            let target = c_string(name)?;
            unix_fs::linkat(
                directory,
                entry.name.as_c_str(),
                directory,
                target.as_c_str(),
                AtFlags::empty(),
            )
            .map_err(|error| format!("candidate output publication failed: {error}"))?;
            // Track the name as soon as linkat succeeds. If the following
            // metadata lookup fails, rollback can still remove this link, but
            // only while it names the held source inode.
            published.push(Published {
                name: target.clone(),
                identity: entry.identity,
            });
            after_publish(index + 1)?;
            let target_metadata = stat_at(directory, &target)
                .map_err(|error| format!("candidate output identity unavailable: {error}"))?;
            let target_identity = file_identity(&target_metadata);
            let target_is_regular = unix_fs::FileType::from_raw_mode(target_metadata.st_mode)
                == unix_fs::FileType::RegularFile;
            if !target_is_regular
                || !target_identity.same_object(entry.identity)
                || target_identity.links != 2
            {
                return Err("candidate published file identity differs from its source".to_owned());
            }
        }
        for entry in temporary {
            if !named_file_matches(directory, &entry.name, entry.identity, 2, false, false) {
                return Err("candidate temporary file changed during publication".to_owned());
            }
            unix_fs::unlinkat(directory, entry.name.as_c_str(), AtFlags::empty())
                .map_err(|error| format!("candidate temporary cleanup failed: {error}"))?;
        }
        for entry in &published {
            if !named_file_matches(directory, &entry.name, entry.identity, 1, false, false) {
                return Err("candidate published file changed before completion".to_owned());
            }
        }
        Ok(())
    })();
    if result.is_err() {
        for entry in &published {
            unlink_matching(directory, &entry.name, entry.identity, -1, true, false);
        }
        cleanup_temporaries(directory, temporary);
    }
    result
}

#[path = "candidate_extract/filesystem.rs"]
mod filesystem;
#[path = "candidate_extract/parse.rs"]
mod parse;
use filesystem::{
    c_string, cleanup_temporaries, create_temp_at, exists_at, file_identity, identity_from_fd,
    named_file_matches, open_archive, open_private_directory, open_trusted_parent, stat_at,
    unlink_matching,
};
use parse::{parse_candidate, validate_names};

#[cfg(test)]
#[path = "candidate_extract_tests.rs"]
mod tests;
