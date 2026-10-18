use super::StageError;
use super::compression::{ArchiveCompression, MAX_ARCHIVE_BYTES, decode_archive, encode_archive};
use super::context::StageContext;
use super::receipt::StageReceiptClaim;
use super::stage::{StageReader, create_stage, open_stage, rewrite_stage};
use rustix::fs::{AtFlags, Mode, OFlags, openat, statat};
use std::ffi::{CString, OsStr, OsString};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::fd::{AsFd, OwnedFd};
use std::path::{Component, Path, PathBuf};

const MAX_STAGE_BYTES: u64 = 20 * 1024 * 1024 * 1024;
const MAX_PATH_BYTES: usize = 4_096;
const BLOCK: usize = 512;

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

/// Gives the verified stage observation produced by an extract operation.
#[derive(Debug)]
pub struct TarOutcome {
    /// Holds a new compact claim after extraction, else `None`.
    pub receipt: Option<StageReceiptClaim>,
}

/// Runs one pinned SDK tar operation without extracting an archive path.
///
/// # Errors
///
/// Returns an error when argv, workspace, archive or stage custody fails validation.
pub fn run_cache_tar(
    context: &StageContext,
    args: &[OsString],
    access: StageAccess<'_>,
    zstd_path: Option<&Path>,
) -> Result<TarOutcome, StageError> {
    if args.len() == 1 && args[0] == "--version" {
        println!("tar (GNU tar) velnor-cache-adapter-v1");
        return Ok(TarOutcome { receipt: None });
    }
    let parsed = parse_args(args)?;
    match parsed.operation {
        Operation::List => {
            let archive = open_archive(context.runner_temp(), parsed.archive)?;
            let mut decoded = decode_archive(archive, parsed.compression, zstd_path)?;
            scan_archive(&mut decoded, context.member_path())?;
            println!("{}", context.member_path());
            Ok(TarOutcome { receipt: None })
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
            Ok(TarOutcome {
                receipt: Some(receipt),
            })
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
            Ok(TarOutcome { receipt: None })
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

fn scan_archive(decoded: &mut File, expected: &str) -> Result<File, StageError> {
    decoded
        .seek(SeekFrom::Start(0))
        .map_err(|_| "cache_tar_archive_read_failed")?;
    let mut payload = tempfile::tempfile().map_err(|_| "cache_tar_temporary_file_failed")?;
    let mut header = [0_u8; BLOCK];
    read_block(decoded, &mut header)?;
    if header.iter().all(|byte| *byte == 0) {
        return Err("cache_tar_member_missing");
    }
    check_checksum(&header)?;
    let (kind, size) = (header[156], parse_number(&header[124..136])?);
    let mut pax_path = None;
    if kind == b'x' {
        if size > 64 * 1024 {
            return Err("cache_tar_metadata_limit");
        }
        let mut body = vec![0_u8; usize::try_from(size).map_err(|_| "cache_tar_metadata_limit")?];
        decoded
            .read_exact(&mut body)
            .map_err(|_| "cache_tar_archive_invalid")?;
        read_padding(decoded, size)?;
        pax_path = Some(parse_pax_path(&body)?);
        read_block(decoded, &mut header)?;
        if header.iter().all(|byte| *byte == 0) {
            return Err("cache_tar_member_missing");
        }
        check_checksum(&header)?;
    }
    if !matches!(header[156], 0 | b'0') {
        return Err("cache_tar_member_type_invalid");
    }
    let size = parse_number(&header[124..136])?;
    if size > MAX_STAGE_BYTES {
        return Err("cache_tar_member_limit");
    }
    let header_path = parse_header_path(&header)?;
    let path = pax_path.unwrap_or(header_path);
    validate_member_path(&path)?;
    if path != expected {
        return Err("cache_tar_member_path_mismatch");
    }
    copy_exact(decoded, &mut payload, size)?;
    read_padding(decoded, size)?;
    read_block(decoded, &mut header)?;
    if !header.iter().all(|byte| *byte == 0) {
        return Err("cache_tar_member_count_invalid");
    }
    read_block(decoded, &mut header)?;
    if !header.iter().all(|byte| *byte == 0) {
        return Err("cache_tar_end_marker_invalid");
    }
    let mut trailing = [0_u8; BLOCK];
    let mut trailing_len = 0_u64;
    loop {
        let count = decoded
            .read(&mut trailing)
            .map_err(|_| "cache_tar_archive_read_failed")?;
        if count == 0 {
            break;
        }
        trailing_len = trailing_len
            .checked_add(u64::try_from(count).map_err(|_| "cache_tar_archive_invalid")?)
            .ok_or("cache_tar_archive_invalid")?;
        if trailing[..count].iter().any(|byte| *byte != 0) {
            return Err("cache_tar_trailing_data_invalid");
        }
    }
    if !trailing_len.is_multiple_of(BLOCK as u64) {
        return Err("cache_tar_trailing_data_invalid");
    }
    payload
        .seek(SeekFrom::Start(0))
        .map_err(|_| "cache_tar_temporary_file_failed")?;
    Ok(payload)
}

fn read_block(reader: &mut File, block: &mut [u8; BLOCK]) -> Result<(), StageError> {
    reader
        .read_exact(block)
        .map_err(|_| "cache_tar_archive_invalid")
}

fn check_checksum(header: &[u8; BLOCK]) -> Result<(), StageError> {
    let expected = parse_number(&header[148..156])?;
    let actual = header
        .iter()
        .enumerate()
        .map(|(index, byte)| {
            if (148..156).contains(&index) {
                usize::from(b' ')
            } else {
                usize::from(*byte)
            }
        })
        .sum::<usize>();
    if usize::try_from(expected).ok() != Some(actual) {
        return Err("cache_tar_checksum_invalid");
    }
    Ok(())
}

fn parse_number(field: &[u8]) -> Result<u64, StageError> {
    if field.first().is_some_and(|byte| byte & 0x80 != 0) {
        if field[0] & 0x40 != 0 {
            return Err("cache_tar_number_invalid");
        }
        let mut value = u64::from(field[0] & 0x7f);
        for byte in &field[1..] {
            value = value
                .checked_mul(256)
                .and_then(|current| current.checked_add(u64::from(*byte)))
                .ok_or("cache_tar_number_invalid")?;
        }
        return Ok(value);
    }
    let trimmed = field
        .iter()
        .copied()
        .skip_while(|byte| matches!(*byte, b' ' | b'\0'))
        .take_while(|byte| !matches!(*byte, b' ' | b'\0'));
    let mut value = 0_u64;
    let mut found = false;
    for byte in trimmed {
        if !(b'0'..=b'7').contains(&byte) {
            return Err("cache_tar_number_invalid");
        }
        value = value
            .checked_mul(8)
            .and_then(|current| current.checked_add(u64::from(byte - b'0')))
            .ok_or("cache_tar_number_invalid")?;
        found = true;
    }
    if !found {
        return Err("cache_tar_number_invalid");
    }
    Ok(value)
}

fn parse_header_path(header: &[u8; BLOCK]) -> Result<String, StageError> {
    let name = field_text(&header[..100])?;
    let prefix = field_text(&header[345..500])?;
    if prefix.is_empty() {
        Ok(name)
    } else if name.is_empty() {
        Err("cache_tar_path_invalid")
    } else {
        Ok(format!("{prefix}/{name}"))
    }
}

fn field_text(field: &[u8]) -> Result<String, StageError> {
    let end = field
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(field.len());
    if field[end..].iter().any(|byte| *byte != 0) {
        return Err("cache_tar_header_invalid");
    }
    let text = std::str::from_utf8(&field[..end]).map_err(|_| "cache_tar_path_invalid")?;
    if text.bytes().any(|byte| byte.is_ascii_control()) {
        return Err("cache_tar_path_invalid");
    }
    Ok(text.to_owned())
}

fn parse_pax_path(body: &[u8]) -> Result<String, StageError> {
    let mut offset = 0_usize;
    let mut path = None;
    while offset < body.len() {
        let space = body[offset..]
            .iter()
            .position(|byte| *byte == b' ')
            .map(|index| offset + index)
            .ok_or("cache_tar_pax_invalid")?;
        if space == offset || !body[offset..space].iter().all(u8::is_ascii_digit) {
            return Err("cache_tar_pax_invalid");
        }
        let length = std::str::from_utf8(&body[offset..space])
            .map_err(|_| "cache_tar_pax_invalid")?
            .parse::<usize>()
            .map_err(|_| "cache_tar_pax_invalid")?;
        let end = offset
            .checked_add(length)
            .filter(|end| *end <= body.len())
            .ok_or("cache_tar_pax_invalid")?;
        if end <= space + 1 || body[end - 1] != b'\n' {
            return Err("cache_tar_pax_invalid");
        }
        let record = &body[space + 1..end - 1];
        let equals = record
            .iter()
            .position(|byte| *byte == b'=')
            .ok_or("cache_tar_pax_invalid")?;
        let key = &record[..equals];
        let value = &record[equals + 1..];
        if key != b"path" || value.is_empty() || value.len() > MAX_PATH_BYTES {
            return Err("cache_tar_pax_invalid");
        }
        let value = std::str::from_utf8(value).map_err(|_| "cache_tar_pax_invalid")?;
        if value.bytes().any(|byte| byte.is_ascii_control()) || path.is_some() {
            return Err("cache_tar_pax_invalid");
        }
        path = Some(value.to_owned());
        offset = end;
    }
    path.ok_or("cache_tar_pax_invalid")
}

fn validate_member_path(path: &str) -> Result<(), StageError> {
    if path.is_empty()
        || path.len() > MAX_PATH_BYTES
        || path.starts_with('/')
        || path
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == 0)
    {
        return Err("cache_tar_path_invalid");
    }
    Ok(())
}

fn copy_exact<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    size: u64,
) -> Result<(), StageError> {
    let mut remaining = size;
    let mut buffer = vec![0_u8; 64 * 1024];
    while remaining > 0 {
        let limit = usize::try_from(remaining.min(buffer.len() as u64))
            .map_err(|_| "cache_tar_member_limit")?;
        let count = reader
            .read(&mut buffer[..limit])
            .map_err(|_| "cache_tar_archive_read_failed")?;
        if count == 0 {
            return Err("cache_tar_member_truncated");
        }
        writer
            .write_all(&buffer[..count])
            .map_err(|_| "cache_tar_temporary_file_failed")?;
        remaining -= u64::try_from(count).map_err(|_| "cache_tar_member_limit")?;
    }
    Ok(())
}

fn read_padding(reader: &mut File, size: u64) -> Result<(), StageError> {
    let padding = (BLOCK as u64 - size % BLOCK as u64) % BLOCK as u64;
    let mut remaining = padding;
    let mut buffer = [0_u8; BLOCK];
    while remaining > 0 {
        let count = usize::try_from(remaining).map_err(|_| "cache_tar_archive_invalid")?;
        reader
            .read_exact(&mut buffer[..count])
            .map_err(|_| "cache_tar_archive_invalid")?;
        if buffer[..count].iter().any(|byte| *byte != 0) {
            return Err("cache_tar_padding_invalid");
        }
        remaining -= count as u64;
    }
    Ok(())
}

fn write_single_member(
    reader: &mut StageReader,
    raw: &mut File,
    member_path: &str,
) -> Result<(), StageError> {
    validate_member_path(member_path)?;
    let (mut prefix, mut name) = member_path.rsplit_once('/').unwrap_or(("", member_path));
    let needs_pax = prefix.len() > 155 || name.len() > 100 || name.is_empty();
    if needs_pax {
        let path_record = pax_path_record(member_path)?;
        let extension_size =
            u64::try_from(path_record.len()).map_err(|_| "cache_tar_pax_invalid")?;
        let extension = make_header("PaxHeaders/cache-stage", "", b'x', extension_size)?;
        raw.write_all(&extension)
            .and_then(|()| raw.write_all(&path_record))
            .map_err(|_| "cache_tar_temporary_file_failed")?;
        write_padding(raw, extension_size)?;
        prefix = "";
        name = "cache-stage.payload";
    }
    if name.is_empty() {
        return Err("cache_tar_path_invalid");
    }
    reader.revalidate().map_err(|_| "cache_tar_stage_invalid")?;
    let size = reader
        .seek(SeekFrom::End(0))
        .map_err(|_| "cache_tar_stage_invalid")?;
    if size > MAX_STAGE_BYTES {
        return Err("cache_tar_member_limit");
    }
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|_| "cache_tar_stage_invalid")?;
    let header = make_header(name, prefix, b'0', size)?;
    raw.write_all(&header)
        .map_err(|_| "cache_tar_temporary_file_failed")?;
    io_copy_bounded(reader, raw, MAX_STAGE_BYTES)?;
    write_padding(raw, size)?;
    write_end_markers(raw)
}

fn make_header(name: &str, prefix: &str, kind: u8, size: u64) -> Result<[u8; BLOCK], StageError> {
    let mut header = [0_u8; BLOCK];
    put_field(&mut header[..100], name.as_bytes())?;
    put_octal(&mut header[100..108], 0o600)?;
    put_octal(&mut header[108..116], 0)?;
    put_octal(&mut header[116..124], 0)?;
    put_number(&mut header[124..136], size)?;
    put_octal(&mut header[136..148], 0)?;
    header[148..156].fill(b' ');
    header[156] = kind;
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    put_field(&mut header[345..500], prefix.as_bytes())?;
    let checksum = header.iter().map(|byte| u64::from(*byte)).sum::<u64>();
    let checksum_text = format!("{checksum:06o}");
    if checksum_text.len() != 6 {
        return Err("cache_tar_header_invalid");
    }
    header[148..154].copy_from_slice(checksum_text.as_bytes());
    header[154] = 0;
    header[155] = b' ';
    Ok(header)
}

fn write_padding(raw: &mut File, size: u64) -> Result<(), StageError> {
    let padding = (BLOCK as u64 - size % BLOCK as u64) % BLOCK as u64;
    if padding > 0 {
        let zeros = [0_u8; BLOCK];
        raw.write_all(&zeros[..usize::try_from(padding).map_err(|_| "cache_tar_member_limit")?])
            .map_err(|_| "cache_tar_temporary_file_failed")?;
    }
    Ok(())
}

fn pax_path_record(path: &str) -> Result<Vec<u8>, StageError> {
    let record = format!("path={path}\n");
    for length in record.len()..=record.len() + 24 {
        let line = format!("{length} {record}");
        if line.len() == length {
            return Ok(line.into_bytes());
        }
    }
    Err("cache_tar_pax_invalid")
}

fn write_end_markers(raw: &mut File) -> Result<(), StageError> {
    raw.write_all(&[0_u8; BLOCK * 2])
        .map_err(|_| "cache_tar_temporary_file_failed")?;
    Ok(())
}

fn put_field(field: &mut [u8], text: &[u8]) -> Result<(), StageError> {
    if text.len() > field.len() {
        return Err("cache_tar_path_invalid");
    }
    field.fill(0);
    field[..text.len()].copy_from_slice(text);
    Ok(())
}

fn put_octal(field: &mut [u8], value: u64) -> Result<(), StageError> {
    let width = field.len().saturating_sub(1);
    let text = format!("{value:0width$o}");
    if text.len() > width {
        return Err("cache_tar_number_invalid");
    }
    field.fill(b'0');
    field[..text.len()].copy_from_slice(text.as_bytes());
    *field.last_mut().ok_or("cache_tar_number_invalid")? = 0;
    Ok(())
}

fn put_number(field: &mut [u8], value: u64) -> Result<(), StageError> {
    if value < 8_u64.pow(u32::try_from(field.len().saturating_sub(1)).unwrap_or(0)) {
        return put_octal(field, value);
    }
    let available_bits = field.len().saturating_mul(8).saturating_sub(2);
    if field.len() < 9
        || (available_bits < u64::BITS as usize && value >= (1_u64 << available_bits))
    {
        return Err("cache_tar_number_invalid");
    }
    field.fill(0);
    let mut remaining = value;
    for byte in field.iter_mut().rev() {
        *byte = u8::try_from(remaining % 256).map_err(|_| "cache_tar_number_invalid")?;
        remaining >>= 8;
    }
    field[0] |= 0x80;
    Ok(())
}

fn io_copy_bounded<R: Read, W: Write>(
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

#[cfg(test)]
mod tests {
    use super::{
        BLOCK, Operation, create_archive, open_archive, parse_args, put_field, put_octal,
        scan_archive, write_single_member,
    };
    use crate::cache_action_stage::{StageContext, create_stage, open_stage};
    use std::ffi::OsString;
    use std::io::{Seek, SeekFrom, Write};
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    fn member(path: &str, kind: u8, body: &[u8]) -> Vec<u8> {
        let mut header = [0_u8; BLOCK];
        put_field(&mut header[..100], path.as_bytes()).expect("test path fits");
        put_octal(&mut header[100..108], 0o600).expect("mode fits");
        put_octal(&mut header[108..116], 0).expect("uid fits");
        put_octal(&mut header[116..124], 0).expect("gid fits");
        put_octal(&mut header[124..136], body.len() as u64).expect("size fits");
        put_octal(&mut header[136..148], 0).expect("mtime fits");
        header[148..156].fill(b' ');
        header[156] = kind;
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");
        let checksum = header.iter().map(|byte| u64::from(*byte)).sum::<u64>();
        let checksum = format!("{checksum:06o}");
        header[148..154].copy_from_slice(checksum.as_bytes());
        header[154] = 0;
        header[155] = b' ';

        let mut archive = header.to_vec();
        archive.extend_from_slice(body);
        archive.resize(archive.len().div_ceil(BLOCK) * BLOCK, 0);
        archive
    }

    fn finish(mut archive: Vec<u8>) -> std::fs::File {
        archive.extend_from_slice(&[0; BLOCK * 2]);
        let mut file = tempfile::tempfile().expect("temporary archive");
        file.write_all(&archive).expect("write archive");
        file.seek(SeekFrom::Start(0)).expect("rewind archive");
        file
    }

    fn archive_path(runner_temp: &Path) -> std::path::PathBuf {
        runner_temp.join("cache.tar")
    }

    #[test]
    fn archive_parent_accepts_a_sticky_shared_temporary_ancestor() {
        let root = tempfile::tempdir().expect("temporary root");
        let shared = root.path().join("shared-temp");
        std::fs::create_dir(&shared).expect("create shared temp");
        std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o1777))
            .expect("protect shared temp with sticky bit");
        let runner_temp_path = shared.join("runner-temp");
        std::fs::create_dir(&runner_temp_path).expect("create private runner temp");
        std::fs::set_permissions(&runner_temp_path, std::fs::Permissions::from_mode(0o700))
            .expect("protect runner temp");
        let runner_temp = std::fs::canonicalize(runner_temp_path).expect("canonical runner temp");
        let archive = archive_path(&runner_temp);
        let resolved = super::resolve_under(&runner_temp, archive.as_os_str())
            .expect("resolve archive under runner temp");
        super::super::stage::open_parent(&resolved).expect("open protected archive parent");

        let (file, _parent, _name) = create_archive(&runner_temp, archive.as_os_str())
            .expect("create archive under sticky shared ancestor");
        drop(file);
        assert_eq!(
            std::fs::metadata(&archive)
                .expect("read created archive metadata")
                .permissions()
                .mode()
                & 0o7777,
            0o600
        );
        open_archive(&runner_temp, archive_path(&runner_temp).as_os_str())
            .expect("open archive under sticky shared ancestor");
    }

    #[test]
    fn archive_path_alias_resolves_under_the_trusted_root_before_descriptor_checks() {
        let root = tempfile::tempdir().expect("temporary root");
        let runner_temp_path = root.path().join("runner-temp");
        std::fs::create_dir(&runner_temp_path).expect("create runner temp");
        std::fs::set_permissions(&runner_temp_path, std::fs::Permissions::from_mode(0o700))
            .expect("protect runner temp");
        let runner_temp = std::fs::canonicalize(runner_temp_path).expect("canonical runner temp");
        let alias = root.path().join("runner-temp-alias");
        std::os::unix::fs::symlink(&runner_temp, &alias).expect("create runner temp alias");
        let archive = alias.join("cache.tar");

        let (file, _parent, _name) = create_archive(&runner_temp, archive.as_os_str())
            .expect("resolve SDK path alias under the trusted root");
        drop(file);
        assert!(runner_temp.join("cache.tar").is_file());
    }

    #[test]
    fn archive_parent_rejects_writable_nonsticky_ancestor_before_creation() {
        let root = tempfile::tempdir().expect("temporary root");
        let shared = root.path().join("shared-temp");
        std::fs::create_dir(&shared).expect("create shared temp");
        std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o777))
            .expect("make ancestor unsafe");
        let runner_temp_path = shared.join("runner-temp");
        std::fs::create_dir(&runner_temp_path).expect("create private runner temp");
        std::fs::set_permissions(&runner_temp_path, std::fs::Permissions::from_mode(0o700))
            .expect("protect runner temp");
        let runner_temp = std::fs::canonicalize(runner_temp_path).expect("canonical runner temp");
        let archive = archive_path(&runner_temp);

        assert!(create_archive(&runner_temp, archive.as_os_str()).is_err());
        assert!(open_archive(&runner_temp, archive.as_os_str()).is_err());
        assert!(!archive.exists());
    }

    #[test]
    fn archive_parent_rejects_unsafe_final_permissions() {
        let root = tempfile::tempdir().expect("temporary root");
        let runner_temp_path = root.path().join("runner-temp");
        std::fs::create_dir(&runner_temp_path).expect("create runner temp");
        std::fs::set_permissions(&runner_temp_path, std::fs::Permissions::from_mode(0o777))
            .expect("make final parent unsafe");
        let runner_temp = std::fs::canonicalize(runner_temp_path).expect("canonical runner temp");
        let archive = archive_path(&runner_temp);

        assert!(create_archive(&runner_temp, archive.as_os_str()).is_err());
        assert!(!archive.exists());
    }

    #[test]
    fn archive_parent_rejects_symlink_replacement() {
        let root = tempfile::tempdir().expect("temporary root");
        let runner_temp_path = root.path().join("runner-temp");
        std::fs::create_dir(&runner_temp_path).expect("create runner temp");
        std::fs::set_permissions(&runner_temp_path, std::fs::Permissions::from_mode(0o700))
            .expect("protect runner temp");
        let runner_temp = std::fs::canonicalize(runner_temp_path).expect("canonical runner temp");
        let moved = root.path().join("runner-temp-moved");
        let archive = archive_path(&runner_temp);
        std::fs::rename(&runner_temp, &moved).expect("move runner temp");
        std::os::unix::fs::symlink(&moved, &runner_temp).expect("replace with symlink");

        assert!(create_archive(&runner_temp, archive.as_os_str()).is_err());
        assert!(open_archive(&runner_temp, archive.as_os_str()).is_err());
        assert!(!moved.join("cache.tar").exists());
    }

    #[test]
    fn archive_create_preserves_an_existing_foreign_file() {
        let root = tempfile::tempdir().expect("temporary root");
        let runner_temp_path = root.path().join("runner-temp");
        std::fs::create_dir(&runner_temp_path).expect("create runner temp");
        let runner_temp = std::fs::canonicalize(runner_temp_path).expect("canonical runner temp");
        let archive = archive_path(&runner_temp);
        std::fs::write(&archive, b"foreign archive").expect("write foreign archive");

        assert!(create_archive(&runner_temp, archive.as_os_str()).is_err());
        assert_eq!(
            std::fs::read(archive).expect("read preserved file"),
            b"foreign archive"
        );
    }

    fn pax_path(path: &str) -> Vec<u8> {
        pax_record("path", path)
    }

    fn pax_record(key: &str, value: &str) -> Vec<u8> {
        let record = format!("{key}={value}\n");
        for length in record.len()..=record.len() + 16 {
            let line = format!("{length} {record}");
            if line.len() == length {
                return line.into_bytes();
            }
        }
        panic!("PAX record length converges");
    }

    #[test]
    fn accepts_pinned_sdk_tar_arguments_and_rejects_unknown_forms() {
        let create = [
            "--posix",
            "-cf",
            "/tmp/archive.tar",
            "--exclude",
            "/tmp/archive.tar",
            "-P",
            "--delay-directory-restore",
            "-C",
            "/workspace",
            "--files-from",
            "manifest.txt",
        ];
        let create = create.map(OsString::from);
        assert_eq!(
            parse_args(&create)
                .expect("valid SDK create args")
                .operation,
            Operation::Create
        );

        let extract = ["-xf", "/tmp/archive.tar", "-P", "-C", "/workspace"].map(OsString::from);
        assert_eq!(
            parse_args(&extract)
                .expect("valid SDK extract args")
                .operation,
            Operation::Extract
        );

        let list = ["-tf", "/tmp/archive.tar", "-P"].map(OsString::from);
        assert_eq!(
            parse_args(&list).expect("valid SDK list args").operation,
            Operation::List
        );

        let long_compressed = [
            "-tf",
            "/tmp/archive.tar",
            "-P",
            "--use-compress-program",
            "unzstd",
            "--long=30",
        ]
        .map(OsString::from);
        assert!(parse_args(&long_compressed).is_ok());

        let compressed_create = [
            "--posix",
            "-cf",
            "/tmp/archive.tar",
            "--exclude",
            "/tmp/archive.tar",
            "-P",
            "-C",
            "/workspace",
            "--files-from",
            "manifest.txt",
            "--use-compress-program",
            "zstdmt",
            "--long=30",
        ]
        .map(OsString::from);
        assert!(parse_args(&compressed_create).is_ok());

        let unsupported =
            ["-xf", "/tmp/archive.tar", "-P", "-C", "/workspace", "-v"].map(OsString::from);
        assert!(parse_args(&unsupported).is_err());
    }

    #[test]
    fn accepts_one_regular_member_with_a_valid_posix_path_record() {
        let expected = "../../runner-temp/stage.payload";
        let mut archive = member("PaxHeaders/stage", b'x', &pax_path(expected));
        archive.extend(member("stage", b'0', b"verified payload"));
        let mut file = finish(archive);
        let mut payload = scan_archive(&mut file, expected).expect("valid one-file archive");
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut payload, &mut bytes).expect("read payload");
        assert_eq!(bytes, b"verified payload");
    }

    #[test]
    fn creates_a_posix_path_record_for_a_long_context_member() {
        let root = tempfile::tempdir().expect("temporary root");
        let workspace = root.path().join("workspace");
        let runner_temp = root.path().join("runner-temp");
        std::fs::create_dir(&workspace).expect("create workspace");
        std::fs::create_dir(&runner_temp).expect("create runner temp");
        let mut nested = runner_temp.clone();
        for index in 0..8 {
            nested.push(format!("nested-{index}-{}", "x".repeat(30)));
            std::fs::create_dir(&nested).expect("create nested stage parent");
        }
        let key = format!("mise-tools-v2-payload-v3-{}", "a".repeat(64));
        let context = StageContext::normal(&nested, &workspace, &key).expect("build stage context");
        assert!(context.member_path().len() > 255);

        let mut writer = create_stage(&context).expect("create stage file");
        writer
            .write_all(b"long path payload")
            .expect("write stage file");
        let claim = writer.finish().expect("finish stage file");
        let mut reader = open_stage(&context, &claim).expect("open verified stage file");
        let mut raw = tempfile::tempfile().expect("temporary tar file");
        write_single_member(&mut reader, &mut raw, context.member_path())
            .expect("write one-file POSIX archive");
        raw.seek(SeekFrom::Start(0)).expect("rewind tar");
        let mut payload =
            scan_archive(&mut raw, context.member_path()).expect("read one-file POSIX archive");
        let mut actual = Vec::new();
        std::io::Read::read_to_end(&mut payload, &mut actual).expect("read payload");
        assert_eq!(actual, b"long path payload");
    }

    #[test]
    fn rejects_foreign_paths_links_multiple_members_and_partial_trailing_data() {
        let expected = "../../runner-temp/stage.payload";
        let mut foreign = finish(member("../../outside", b'0', b"x"));
        assert!(scan_archive(&mut foreign, expected).is_err());

        let mut absolute = finish(member("/outside", b'0', b"x"));
        assert!(scan_archive(&mut absolute, expected).is_err());

        let mut link = finish(member(expected, b'2', b"target"));
        assert!(scan_archive(&mut link, expected).is_err());

        let mut device = finish(member(expected, b'3', b""));
        assert!(scan_archive(&mut device, expected).is_err());

        let mut unsupported_pax = member(
            "PaxHeaders/stage",
            b'x',
            &pax_record("linkpath", "../../outside"),
        );
        unsupported_pax.extend(member("stage", b'0', b"x"));
        let mut unsupported_pax = finish(unsupported_pax);
        assert!(scan_archive(&mut unsupported_pax, expected).is_err());

        let mut two_members = member("first", b'0', b"x");
        two_members.extend(member("second", b'0', b"y"));
        let mut multiple = finish(two_members);
        assert!(scan_archive(&mut multiple, "first").is_err());

        let mut partial = tempfile::tempfile().expect("temporary archive");
        partial
            .write_all(&member(expected, b'0', b"x"))
            .expect("write member");
        partial
            .write_all(&[0; BLOCK * 2])
            .expect("write end markers");
        partial.write_all(&[0]).expect("write incomplete block");
        partial.seek(SeekFrom::Start(0)).expect("rewind archive");
        assert!(scan_archive(&mut partial, expected).is_err());
    }
}
