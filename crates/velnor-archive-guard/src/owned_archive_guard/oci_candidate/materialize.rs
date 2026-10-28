// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::Path,
};

use sha2::Digest;

use super::{
    OciCandidateError, OciCandidateEvidence, OciMaterializeError, Result,
    archive::{Blob, OciArchive, authenticate_archive, hex, load_archive},
    graph::inspect_graph_preserving_archive,
};

type MaterializeResult<T> = std::result::Result<T, OciMaterializeError>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DirectoryIdentity {
    device: u64,
    inode: u64,
}

pub(super) fn materialize_oci_candidate(
    archive_path: &Path,
    destination: &Path,
    expected: &OciCandidateEvidence,
) -> MaterializeResult<OciCandidateEvidence> {
    let archive = load_archive(archive_path)?;
    let (evidence, mut archive) = inspect_graph_preserving_archive(archive)?;
    if &evidence != expected {
        return Err(OciCandidateError("oci_candidate_evidence_mismatch").into());
    }
    let identity = create_destination(destination)?;
    let result = materialize_contents(&mut archive, destination, &evidence);
    match result {
        Ok(()) => Ok(evidence),
        Err(error) => {
            cleanup_destination(destination, identity)?;
            Err(error.into())
        }
    }
}

fn create_destination(path: &Path) -> MaterializeResult<DirectoryIdentity> {
    let mut builder = fs::DirBuilder::new();
    builder.mode(0o700);
    match builder.create(path) {
        Ok(()) => match directory_identity(path) {
            Ok(identity) => Ok(identity),
            Err(error) => {
                if fs::remove_dir(path).is_err() {
                    return Err(cleanup_failure(path));
                }
                Err(error.into())
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            Err(OciCandidateError("oci_projection_destination_exists").into())
        }
        Err(_) => Err(OciCandidateError("oci_projection_destination_create").into()),
    }
}

fn directory_identity(path: &Path) -> Result<DirectoryIdentity> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| OciCandidateError("oci_projection_destination_stat"))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(OciCandidateError("oci_projection_destination_type"));
    }
    Ok(DirectoryIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

fn materialize_contents(
    archive: &mut OciArchive,
    destination: &Path,
    evidence: &OciCandidateEvidence,
) -> Result<()> {
    write_metadata(&destination.join("oci-layout"), &archive.layout)?;
    write_metadata(&destination.join("index.json"), &archive.index)?;
    let blobs = destination.join("blobs");
    create_private_directory(&blobs)?;
    let sha256 = blobs.join("sha256");
    create_private_directory(&sha256)?;
    let blobs = archive
        .blobs
        .iter()
        .map(|(digest, blob)| (digest.clone(), *blob))
        .collect::<Vec<_>>();
    for (digest, blob) in blobs {
        write_blob(archive, &blob, &digest, &sha256.join(&digest))?;
    }
    let (digest, size) = authenticate_archive(archive)?;
    if digest != evidence.archive_sha256 || size != evidence.archive_size_bytes {
        return Err(OciCandidateError("oci_projection_archive_changed"));
    }
    Ok(())
}

fn create_private_directory(path: &Path) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.mode(0o700);
    builder
        .create(path)
        .map_err(|_| OciCandidateError("oci_projection_directory_create"))
}

fn create_private_file(path: &Path) -> Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|_| OciCandidateError("oci_projection_file_create"))
}

fn write_metadata(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut output = create_private_file(path)?;
    output
        .write_all(bytes)
        .map_err(|_| OciCandidateError("oci_projection_metadata_write"))?;
    output
        .sync_all()
        .map_err(|_| OciCandidateError("oci_projection_metadata_sync"))?;
    output
        .seek(SeekFrom::Start(0))
        .map_err(|_| OciCandidateError("oci_projection_metadata_seek"))?;
    let mut copied = Vec::with_capacity(bytes.len());
    output
        .read_to_end(&mut copied)
        .map_err(|_| OciCandidateError("oci_projection_metadata_read"))?;
    if copied != bytes {
        return Err(OciCandidateError("oci_projection_metadata_mismatch"));
    }
    Ok(())
}

fn write_blob(
    archive: &mut OciArchive,
    blob: &Blob,
    digest: &str,
    destination: &Path,
) -> Result<()> {
    archive
        .file
        .seek(SeekFrom::Start(blob.offset))
        .map_err(|_| OciCandidateError("oci_projection_blob_seek"))?;
    let mut output = create_private_file(destination)?;
    let mut remaining = blob.size;
    let mut buffer = vec![0; 128 * 1024];
    let buffer_size =
        u64::try_from(buffer.len()).map_err(|_| OciCandidateError("oci_projection_blob_size"))?;
    while remaining > 0 {
        let amount = usize::try_from(remaining.min(buffer_size))
            .map_err(|_| OciCandidateError("oci_projection_blob_size"))?;
        archive
            .file
            .read_exact(&mut buffer[..amount])
            .map_err(|_| OciCandidateError("oci_projection_blob_read"))?;
        output
            .write_all(&buffer[..amount])
            .map_err(|_| OciCandidateError("oci_projection_blob_write"))?;
        remaining -=
            u64::try_from(amount).map_err(|_| OciCandidateError("oci_projection_blob_size"))?;
    }
    output
        .sync_all()
        .map_err(|_| OciCandidateError("oci_projection_blob_sync"))?;
    let (copied_digest, copied_size) = hash_copied_blob(&mut output, blob.size)?;
    if copied_digest != digest || copied_size != blob.size {
        return Err(OciCandidateError("oci_projection_blob_mismatch"));
    }
    Ok(())
}

fn hash_copied_blob(file: &mut File, limit: u64) -> Result<(String, u64)> {
    file.seek(SeekFrom::Start(0))
        .map_err(|_| OciCandidateError("oci_projection_blob_seek"))?;
    let mut digest = sha2::Sha256::new();
    let mut size = 0_u64;
    let mut buffer = vec![0; 128 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| OciCandidateError("oci_projection_blob_read"))?;
        if count == 0 {
            break;
        }
        size = size
            .checked_add(
                u64::try_from(count).map_err(|_| OciCandidateError("oci_projection_blob_size"))?,
            )
            .filter(|actual| *actual <= limit)
            .ok_or(OciCandidateError("oci_projection_blob_size"))?;
        sha2::Digest::update(&mut digest, &buffer[..count]);
    }
    Ok((hex(&sha2::Digest::finalize(digest)), size))
}

fn cleanup_destination(path: &Path, expected: DirectoryIdentity) -> MaterializeResult<()> {
    let actual = directory_identity(path).map_err(|_| cleanup_failure(path))?;
    if actual != expected {
        return Err(cleanup_failure(path));
    }
    fs::remove_dir_all(path).map_err(|_| cleanup_failure(path))
}

fn cleanup_failure(path: &Path) -> OciMaterializeError {
    OciMaterializeError::CleanupFailure {
        destination: path.to_path_buf(),
    }
}
