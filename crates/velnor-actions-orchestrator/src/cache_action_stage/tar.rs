use super::StageError;
use super::compression::{ArchiveCompression, MAX_ARCHIVE_BYTES, decode_archive, encode_archive};
use super::context::StageContext;
use super::receipt::StageReceiptClaim;
use super::stage::{create_stage, open_stage, rewrite_stage};
use rustix::fs::{AtFlags, Mode, OFlags, openat, statat};
use std::ffi::{CString, OsStr, OsString};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::fd::{AsFd, OwnedFd};
use std::path::{Component, Path, PathBuf};

const MAX_STAGE_BYTES: u64 = 20 * 1024 * 1024 * 1024;
const MAX_PATH_BYTES: usize = 4_096;
const BLOCK: usize = 512;

use self::format::scan_archive;
use self::writer::write_single_member;
#[cfg(test)]
use self::writer::{put_field, put_octal};

#[path = "tar_format.rs"]
mod format;
#[path = "tar_outcome.rs"]
mod outcome;
pub use outcome::TarOutcome;
#[cfg(test)]
#[path = "tar_tests.rs"]
mod tests;
#[path = "tar_write.rs"]
mod writer;

/// Describes the stage authority for one SDK tar operation.
#[derive(Clone, Copy, Debug)]
pub enum StageAccess<'a> {
    /// Create a new file with exclusive ownership.
    New,
    /// Read a file bound by a prior claim.
    Read(&'a StageReceiptClaim),
    /// Rewrite only a file bound by a prior claim.
    Rewrite(&'a StageReceiptClaim),
}

/// Runs one pinned SDK tar operation without extracting an archive path.
///
/// # Errors
///
/// The function gives an error if the argument-vector, workspace, archive, or stage-file checks give unsatisfactory results.
pub fn run_cache_tar(
    context: &StageContext,
    args: &[OsString],
    access: StageAccess<'_>,
    zstd_path: Option<&Path>,
) -> Result<TarOutcome, StageError> {
    if args.len() == 1 && args[0] == "--version" {
        println!("tar (GNU tar) velnor-cache-adapter-v1");
        return Ok(TarOutcome::without_observation());
    }
    let parsed = parse_args(args)?;
    match parsed.operation {
        Operation::List => {
            let archive = open_archive(context.runner_temp(), parsed.archive)?;
            let mut decoded = decode_archive(archive, parsed.compression, zstd_path)?;
            scan_archive(&mut decoded, context.member_path())?;
            println!("{}", context.member_path());
            TarOutcome::observed(None, parsed.compression)
        }
        Operation::Extract => {
            if !matches_workspace(parsed.change_directory.as_deref(), context.workspace()) {
                return Err("cache_tar_workspace_mismatch");
            }
            let archive = open_archive(context.runner_temp(), parsed.archive)?;
            let mut decoded = decode_archive(archive, parsed.compression, zstd_path)?;
            let mut payload = scan_archive(&mut decoded, context.member_path())?;
            let mut writer = match access {
                StageAccess::New => create_stage(context)?,
                StageAccess::Rewrite(claim) => rewrite_stage(context, claim)?,
                StageAccess::Read(_) => return Err("cache_tar_stage_access_invalid"),
            };
            io_copy_bounded(&mut payload, &mut writer, MAX_STAGE_BYTES)?;
            let receipt = writer.finish()?;
            TarOutcome::observed(Some(receipt), parsed.compression)
        }
        Operation::Create => {
            if !matches_workspace(parsed.change_directory.as_deref(), context.workspace()) {
                return Err("cache_tar_workspace_mismatch");
            }
            let manifest = read_manifest(&parsed.manifest)?;
            if manifest != context.member_path() {
                return Err("cache_tar_manifest_mismatch");
            }
            let StageAccess::Read(claim) = access else {
                return Err("cache_tar_stage_access_invalid");
            };
            let mut reader = open_stage(context, claim)?;
            let mut raw = tempfile::tempfile().map_err(|_| "cache_tar_temporary_file_failed")?;
            write_single_member(&mut reader, &mut raw, context.member_path())?;
            reader.revalidate()?;
            raw.seek(SeekFrom::Start(0))
                .map_err(|_| "cache_tar_temporary_file_failed")?;
            let (mut output, _parent, _name) =
                create_archive(context.runner_temp(), parsed.archive)?;
            encode_archive(raw, &mut output, parsed.compression, zstd_path)?;
            output
                .flush()
                .and_then(|()| output.sync_all())
                .map_err(|_| "cache_tar_archive_write_failed")?;
            TarOutcome::observed(None, parsed.compression)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Operation {
    Create,
    List,
    Extract,
}

struct ParsedArgs<'a> {
    operation: Operation,
    archive: &'a OsStr,
    change_directory: Option<PathBuf>,
    manifest: PathBuf,
    compression: ArchiveCompression,
}

fn parse_args(args: &[OsString]) -> Result<ParsedArgs<'_>, StageError> {
    let first = args.first().ok_or("cache_tar_arguments_invalid")?;
    let (operation, mut index) = match first.to_str() {
        Some("--posix") if args.get(1).is_some_and(|value| value == "-cf") => {
            (Operation::Create, 2)
        }
        Some("-tf") => (Operation::List, 1),
        Some("-xf") => (Operation::Extract, 1),
        _ => return Err("cache_tar_arguments_invalid"),
    };
    let archive = args.get(index).ok_or("cache_tar_arguments_invalid")?;
    index += 1;
    let mut exclude = None;
    let mut change_directory = None;
    let mut manifest = None;
    let mut compression = ArchiveCompression::None;
    let mut saw_preserve_paths = false;
    while index < args.len() {
        let flag = args[index].to_str().ok_or("cache_tar_arguments_invalid")?;
        match flag {
            "--exclude" if operation == Operation::Create && exclude.is_none() => {
                index += 1;
                exclude = Some(
                    args.get(index)
                        .ok_or("cache_tar_arguments_invalid")?
                        .as_os_str(),
                );
            }
            "-P" if !saw_preserve_paths => saw_preserve_paths = true,
            "-C" if operation != Operation::List && change_directory.is_none() => {
                index += 1;
                change_directory = Some(clean_path_arg(
                    args.get(index).ok_or("cache_tar_arguments_invalid")?,
                )?);
            }
            "--files-from" if operation == Operation::Create && manifest.is_none() => {
                index += 1;
                manifest = Some(clean_manifest_arg(
                    args.get(index).ok_or("cache_tar_arguments_invalid")?,
                )?);
            }
            "--delay-directory-restore" => {}
            "-z" if compression == ArchiveCompression::None => {
                compression = ArchiveCompression::Gzip;
            }
            "--use-compress-program" if compression == ArchiveCompression::None => {
                compression = parse_compression_program(args, &mut index, operation)?;
            }
            _ => return Err("cache_tar_arguments_invalid"),
        }
        index += 1;
    }
    if !saw_preserve_paths {
        return Err("cache_tar_arguments_invalid");
    }
    if operation == Operation::Create {
        let Some(exclude) = exclude else {
            return Err("cache_tar_arguments_invalid");
        };
        if exclude != archive || manifest.is_none() || change_directory.is_none() {
            return Err("cache_tar_arguments_invalid");
        }
    }
    if compression == ArchiveCompression::None {
        return Err("cache_tar_compression_missing");
    }
    Ok(ParsedArgs {
        operation,
        archive,
        change_directory,
        manifest: manifest.unwrap_or_default(),
        compression,
    })
}

fn parse_compression_program(
    args: &[OsString],
    index: &mut usize,
    operation: Operation,
) -> Result<ArchiveCompression, StageError> {
    *index += 1;
    let program = args
        .get(*index)
        .and_then(|value| value.to_str())
        .ok_or("cache_tar_arguments_invalid")?;
    let long_program = (operation == Operation::Create && program == "zstdmt")
        || (operation != Operation::Create && program == "unzstd");
    if long_program
        && args
            .get(*index + 1)
            .is_some_and(|value| value == "--long=30")
    {
        *index += 1;
        return Ok(ArchiveCompression::ZstdLong);
    }
    match (operation, program) {
        (Operation::Create, "zstdmt") | (Operation::List | Operation::Extract, "unzstd") => {
            Ok(ArchiveCompression::Zstd)
        }
        _ => Err("cache_tar_compression_invalid"),
    }
}

fn clean_path_arg(value: &OsStr) -> Result<PathBuf, StageError> {
    let path = PathBuf::from(value);
    if !path.is_absolute() || path.as_os_str().len() > MAX_PATH_BYTES {
        return Err("cache_tar_path_invalid");
    }
    for component in path.components() {
        if matches!(
            component,
            Component::ParentDir | Component::CurDir | Component::Prefix(_)
        ) {
            return Err("cache_tar_path_invalid");
        }
    }
    Ok(path)
}

fn matches_workspace(input: Option<&Path>, workspace: &Path) -> bool {
    input.is_some_and(|path| {
        std::fs::canonicalize(path).is_ok_and(|canonical| canonical == workspace)
    })
}

fn clean_manifest_arg(value: &OsStr) -> Result<PathBuf, StageError> {
    if value != "manifest.txt" {
        return Err("cache_tar_manifest_invalid");
    }
    Ok(PathBuf::from(value))
}

fn read_manifest(path: &Path) -> Result<String, StageError> {
    let bytes = std::fs::read(path).map_err(|_| "cache_tar_manifest_invalid")?;
    if bytes.len() > MAX_PATH_BYTES || bytes.iter().any(|byte| matches!(*byte, 0 | b'\r' | b'\n')) {
        return Err("cache_tar_manifest_invalid");
    }
    String::from_utf8(bytes).map_err(|_| "cache_tar_manifest_invalid")
}

fn open_archive(runner_temp: &Path, input: &OsStr) -> Result<File, StageError> {
    let path = resolve_under(runner_temp, input)?;
    let (parent, name) = open_parent(&path)?;
    let descriptor = openat(
        &parent,
        name.as_c_str(),
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| "cache_tar_archive_open_failed")?;
    let file = File::from(descriptor);
    let stat = rustix::fs::fstat(file.as_fd()).map_err(|_| "cache_tar_archive_open_failed")?;
    let path_stat = statat(&parent, name.as_c_str(), AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|_| "cache_tar_archive_open_failed")?;
    if rustix::fs::FileType::from_raw_mode(stat.st_mode) != rustix::fs::FileType::RegularFile
        || stat.st_uid != crate::process_owner::effective_uid()
        || stat.st_nlink != 1
        || stat.st_mode & 0o7000 != 0
        || u64::try_from(stat.st_size).unwrap_or(u64::MAX) > MAX_ARCHIVE_BYTES
        || stat.st_dev != path_stat.st_dev
        || stat.st_ino != path_stat.st_ino
    {
        return Err("cache_tar_archive_invalid");
    }
    Ok(file)
}

fn create_archive(
    runner_temp: &Path,
    input: &OsStr,
) -> Result<(File, OwnedFd, CString), StageError> {
    let path = resolve_under(runner_temp, input)?;
    let (parent, name) = open_parent(&path)?;
    let descriptor = openat(
        &parent,
        name.as_c_str(),
        OFlags::WRONLY
            | OFlags::CREATE
            | OFlags::EXCL
            | OFlags::NOFOLLOW
            | OFlags::NONBLOCK
            | OFlags::CLOEXEC,
        Mode::from_raw_mode(0o600),
    )
    .map_err(|_| "cache_tar_archive_create_failed")?;
    let file = File::from(descriptor);
    rustix::fs::fchmod(file.as_fd(), Mode::from_raw_mode(0o600))
        .map_err(|_| "cache_tar_archive_create_failed")?;
    Ok((file, parent, name))
}

fn resolve_under(runner_temp: &Path, input: &OsStr) -> Result<PathBuf, StageError> {
    let path = PathBuf::from(input);
    let path = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .map_err(|_| "cache_tar_path_invalid")?
            .join(path)
    };
    let root = lexical_absolute(runner_temp)?;
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::CurDir | Component::Prefix(_)
        )
    }) {
        return Err("cache_tar_path_invalid");
    }
    let path = lexical_absolute(&path)?;
    let parent = path.parent().ok_or("cache_tar_path_invalid")?;
    let parent = std::fs::canonicalize(parent).map_err(|_| "cache_tar_path_invalid")?;
    let name = path.file_name().ok_or("cache_tar_path_invalid")?;
    let path = parent.join(name);
    if path == root || !path.starts_with(&root) {
        return Err("cache_tar_path_invalid");
    }
    Ok(path)
}

fn lexical_absolute(path: &Path) -> Result<PathBuf, StageError> {
    if !path.is_absolute() {
        return Err("cache_tar_path_invalid");
    }
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir => result.push("/"),
            Component::Normal(value) => result.push(value),
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            Component::Prefix(_) => return Err("cache_tar_path_invalid"),
        }
    }
    Ok(result)
}

fn open_parent(path: &Path) -> Result<(OwnedFd, CString), StageError> {
    super::stage::open_parent(path).map_err(|_| "cache_tar_path_invalid")
}

pub(super) fn io_copy_bounded<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    limit: u64,
) -> Result<(), StageError> {
    let mut copied = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|_| "cache_tar_stage_read_failed")?;
        if count == 0 {
            break;
        }
        copied = copied
            .checked_add(u64::try_from(count).map_err(|_| "cache_tar_member_limit")?)
            .ok_or("cache_tar_member_limit")?;
        if copied > limit {
            return Err("cache_tar_member_limit");
        }
        writer
            .write_all(&buffer[..count])
            .map_err(|_| "cache_tar_stage_write_failed")?;
    }
    Ok(())
}
