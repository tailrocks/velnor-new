use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    os::unix::fs::MetadataExt,
    path::Path,
};

use rustix::fs::{Mode, OFlags, open};
use sha2::Digest;
use tar::EntryType;

use super::archive_tar_framing::scan_archive_extensions;
use super::{
    MAX_OCI_CANDIDATE_ARCHIVE_BYTES, MAX_OCI_CANDIDATE_ARCHIVE_MEMBERS,
    MAX_OCI_CANDIDATE_BLOB_BYTES, OciCandidateError, Result,
};

const METADATA_LIMIT: usize = 1024 * 1024;
const BLOB_LIMIT: usize = 64;

pub(super) struct Blob {
    pub(super) offset: u64,
    pub(super) size: u64,
}

pub(super) struct OciArchive {
    pub(super) path: std::path::PathBuf,
    pub(super) file: File,
    pub(super) initial: fs::Metadata,
    pub(super) layout: Vec<u8>,
    pub(super) index: Vec<u8>,
    pub(super) blobs: BTreeMap<String, Blob>,
}

struct ArchiveEntries {
    layout: Option<Vec<u8>>,
    index: Option<Vec<u8>>,
    blobs: BTreeMap<String, Blob>,
}

pub(super) fn load_archive(path: &Path) -> Result<OciArchive> {
    let mut file = open_regular(path)?;
    let initial = file
        .metadata()
        .map_err(|_| OciCandidateError("archive_stat"))?;
    if initial.len() > MAX_OCI_CANDIDATE_ARCHIVE_BYTES {
        return Err(OciCandidateError("archive_size_limit"));
    }
    scan_archive_extensions(&mut file, initial.len())?;
    file.seek(SeekFrom::Start(0))
        .map_err(|_| OciCandidateError("archive_seek"))?;
    let mut archive = tar::Archive::new(file);
    let entries = read_archive_entries(&mut archive)?;
    let mut file = archive.into_inner();
    file.seek(SeekFrom::Start(0))
        .map_err(|_| OciCandidateError("archive_seek"))?;
    Ok(OciArchive {
        path: path.to_path_buf(),
        file,
        initial,
        layout: entries
            .layout
            .ok_or(OciCandidateError("archive_layout_missing"))?,
        index: entries
            .index
            .ok_or(OciCandidateError("archive_index_missing"))?,
        blobs: entries.blobs,
    })
}

fn read_archive_entries(archive: &mut tar::Archive<File>) -> Result<ArchiveEntries> {
    let entries = archive
        .entries()
        .map_err(|_| OciCandidateError("archive_tar"))?;
    let mut names = std::collections::BTreeSet::new();
    let mut content_size = 0_u64;
    let mut layout = None;
    let mut index = None;
    let mut blobs = BTreeMap::new();
    for (count, result) in entries.enumerate() {
        if count >= MAX_OCI_CANDIDATE_ARCHIVE_MEMBERS {
            return Err(OciCandidateError("archive_member_limit"));
        }
        let mut entry = result.map_err(|_| OciCandidateError("archive_entry"))?;
        let raw_name = entry
            .path()
            .map_err(|_| OciCandidateError("archive_path"))?;
        let name = raw_name
            .to_str()
            .ok_or(OciCandidateError("archive_path_utf8"))?;
        let name = if entry.header().entry_type().is_dir() {
            name.strip_suffix('/').unwrap_or(name)
        } else {
            name
        };
        validate_member_name(name)?;
        if !names.insert(name.to_owned()) {
            return Err(OciCandidateError("archive_duplicate_path"));
        }
        let entry_type = entry.header().entry_type();
        if entry_type.is_dir() {
            if !matches!(name, "blobs" | "blobs/sha256") {
                return Err(OciCandidateError("archive_unexpected_directory"));
            }
            continue;
        }
        if entry_type != EntryType::Regular {
            return Err(OciCandidateError("archive_nonregular_entry"));
        }
        let size = entry.size();
        if size > MAX_OCI_CANDIDATE_BLOB_BYTES {
            return Err(OciCandidateError("archive_blob_size_limit"));
        }
        content_size = content_size
            .checked_add(size)
            .filter(|value| *value <= MAX_OCI_CANDIDATE_ARCHIVE_BYTES)
            .ok_or(OciCandidateError("archive_content_limit"))?;
        match name {
            "oci-layout" => layout = Some(read_metadata(&mut entry, size)?),
            "index.json" => index = Some(read_metadata(&mut entry, size)?),
            path => {
                let digest = path
                    .strip_prefix("blobs/sha256/")
                    .ok_or(OciCandidateError("archive_unexpected_path"))?;
                if !is_hex_digest(digest) || blobs.len() >= BLOB_LIMIT {
                    return Err(OciCandidateError("archive_blob_path_or_count"));
                }
                blobs.insert(
                    digest.to_owned(),
                    Blob {
                        offset: entry.raw_file_position(),
                        size,
                    },
                );
            }
        }
    }
    Ok(ArchiveEntries {
        layout,
        index,
        blobs,
    })
}

pub(super) fn authenticate_archive(archive: &mut OciArchive) -> Result<(String, u64)> {
    verify_file_identity(&archive.path, &archive.file, &archive.initial)?;
    archive
        .file
        .seek(SeekFrom::Start(0))
        .map_err(|_| OciCandidateError("archive_seek"))?;
    let mut digest = sha2::Sha256::new();
    let mut size = 0_u64;
    let mut buffer = vec![0_u8; 128 * 1024];
    loop {
        let count = archive
            .file
            .read(&mut buffer)
            .map_err(|_| OciCandidateError("archive_read"))?;
        if count == 0 {
            break;
        }
        size = size
            .checked_add(u64::try_from(count).map_err(|_| OciCandidateError("archive_size"))?)
            .filter(|value| *value <= archive.initial.len())
            .ok_or(OciCandidateError("archive_size"))?;
        sha2::Digest::update(&mut digest, &buffer[..count]);
    }
    if size != archive.initial.len() {
        return Err(OciCandidateError("archive_size_changed"));
    }
    verify_file_identity(&archive.path, &archive.file, &archive.initial)?;
    Ok((hex(&sha2::Digest::finalize(digest)), size))
}

pub(super) fn read_blob(file: &mut File, blob: &Blob, limit: u64) -> Result<Vec<u8>> {
    if blob.size > limit {
        return Err(OciCandidateError("blob_read_limit"));
    }
    let size = usize::try_from(blob.size).map_err(|_| OciCandidateError("blob_size_conversion"))?;
    file.seek(SeekFrom::Start(blob.offset))
        .map_err(|_| OciCandidateError("blob_seek"))?;
    let mut bytes = vec![0; size];
    file.read_exact(&mut bytes)
        .map_err(|_| OciCandidateError("blob_truncated"))?;
    Ok(bytes)
}

pub(super) fn hash_blob(file: &mut File, blob: &Blob) -> Result<(String, u64)> {
    file.seek(SeekFrom::Start(blob.offset))
        .map_err(|_| OciCandidateError("blob_seek"))?;
    let mut remaining = blob.size;
    let mut buffer = vec![0; 128 * 1024];
    let mut digest = sha2::Sha256::new();
    while remaining > 0 {
        let buffer_size =
            u64::try_from(buffer.len()).map_err(|_| OciCandidateError("blob_size_conversion"))?;
        let amount = usize::try_from(remaining.min(buffer_size))
            .map_err(|_| OciCandidateError("blob_size_conversion"))?;
        file.read_exact(&mut buffer[..amount])
            .map_err(|_| OciCandidateError("blob_truncated"))?;
        sha2::Digest::update(&mut digest, &buffer[..amount]);
        remaining -=
            u64::try_from(amount).map_err(|_| OciCandidateError("blob_size_conversion"))?;
    }
    Ok((hex(&sha2::Digest::finalize(digest)), blob.size))
}

fn read_metadata(entry: &mut tar::Entry<'_, File>, size: u64) -> Result<Vec<u8>> {
    let size = usize::try_from(size).map_err(|_| OciCandidateError("metadata_size_conversion"))?;
    if size > METADATA_LIMIT {
        return Err(OciCandidateError("metadata_size_limit"));
    }
    let mut bytes = Vec::with_capacity(size);
    entry
        .read_to_end(&mut bytes)
        .map_err(|_| OciCandidateError("metadata_read"))?;
    if bytes.len() != size {
        return Err(OciCandidateError("metadata_truncated"));
    }
    Ok(bytes)
}

fn open_regular(path: &Path) -> Result<File> {
    let before = fs::symlink_metadata(path).map_err(|_| OciCandidateError("archive_stat"))?;
    if !before.is_file() || before.file_type().is_symlink() {
        return Err(OciCandidateError("archive_not_regular"));
    }
    let descriptor = open(
        path,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .map_err(|_| OciCandidateError("archive_open"))?;
    let file = File::from(descriptor);
    let opened = file
        .metadata()
        .map_err(|_| OciCandidateError("archive_stat"))?;
    if !opened.is_file() || opened.dev() != before.dev() || opened.ino() != before.ino() {
        return Err(OciCandidateError("archive_identity_changed"));
    }
    Ok(file)
}

fn verify_file_identity(path: &Path, file: &File, initial: &fs::Metadata) -> Result<()> {
    let current = file
        .metadata()
        .map_err(|_| OciCandidateError("archive_stat"))?;
    let path_metadata =
        fs::symlink_metadata(path).map_err(|_| OciCandidateError("archive_stat"))?;
    if current.len() != initial.len()
        || current.dev() != initial.dev()
        || current.ino() != initial.ino()
        || current.mtime() != initial.mtime()
        || current.mtime_nsec() != initial.mtime_nsec()
        || current.ctime() != initial.ctime()
        || current.ctime_nsec() != initial.ctime_nsec()
        || path_metadata.dev() != initial.dev()
        || path_metadata.ino() != initial.ino()
        || !path_metadata.is_file()
        || path_metadata.file_type().is_symlink()
    {
        return Err(OciCandidateError("archive_identity_changed"));
    }
    Ok(())
}

fn validate_member_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 1024
        || name.starts_with('/')
        || name
            .split('/')
            .any(|part| part.is_empty() || part == ".." || part == ".")
        || name.contains('\\')
        || name.as_bytes().contains(&0)
    {
        return Err(OciCandidateError("archive_path_unsafe"));
    }
    Ok(())
}

fn is_hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(DIGITS[usize::from(byte >> 4)]));
        encoded.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    encoded
}
