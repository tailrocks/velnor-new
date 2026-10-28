// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};
use velnor_actions_contract::ImageCandidateOci;
use velnor_actions_mise::CheckDeadline;

use crate::{OrchestratorError, internal::internal};

use super::{Descriptor, Manifest};

const COMPARE_CHUNK_BYTES: usize = 32 * 1024;

pub(super) fn verify_remote_bytes(
    layout: &Path,
    docker_config: &Path,
    repository: &str,
    expected_digest: &str,
    expected: &ImageCandidateOci,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    check_deadline(deadline)?;
    if expected_digest != expected.platform_manifest_digest
        || expected.platform_manifest_size_bytes > super::OUTPUT_LIMIT as u64
    {
        return Err(internal("image_publication_local_manifest_identity"));
    }
    let manifest_path = local_blob_path(layout, expected_digest)?;
    let bytes = read_manifest(
        &manifest_path,
        expected.platform_manifest_size_bytes,
        deadline,
    )?;
    if super::digest(&bytes) != expected_digest {
        return Err(internal("image_publication_local_manifest_digest"));
    }
    let manifest: Manifest =
        serde_json::from_slice(&bytes).map_err(|_| internal("image_publication_local_manifest"))?;
    if manifest.schema_version != 2
        || manifest.config.digest != expected.image_configuration_digest
        || manifest.config.size != expected.image_configuration_size_bytes
        || manifest.layers.len() != expected.layer_digests.len()
    {
        return Err(internal("image_publication_local_manifest_identity"));
    }
    for (layer, expected_layer) in manifest.layers.iter().zip(&expected.layer_digests) {
        if &layer.digest != expected_layer {
            return Err(internal("image_publication_local_manifest_identity"));
        }
    }
    let immutable_reference = format!("{repository}@{expected_digest}");
    let remote_manifest = super::crane_run(
        docker_config,
        vec![
            OsString::from("manifest"),
            OsString::from(immutable_reference),
        ],
        deadline,
    )?;
    if !remote_manifest.success
        || remote_manifest.code != Some(0)
        || remote_manifest.signal.is_some()
        || remote_manifest.stdout.len() as u64 != expected.platform_manifest_size_bytes
        || super::digest(&remote_manifest.stdout) != expected_digest
        || remote_manifest.stdout != bytes
    {
        return Err(internal("image_publication_remote_manifest"));
    }
    let publication_dir = layout
        .parent()
        .ok_or_else(|| internal("image_publication_readback_directory"))?;
    for (index, descriptor) in std::iter::once(&manifest.config)
        .chain(manifest.layers.iter())
        .enumerate()
    {
        verify_blob(
            publication_dir,
            layout,
            docker_config,
            repository,
            index,
            descriptor,
            deadline,
        )?;
    }
    Ok(())
}

fn read_manifest(
    path: &Path,
    expected_size: u64,
    deadline: CheckDeadline,
) -> Result<Vec<u8>, OrchestratorError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| internal("image_publication_local_manifest"))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() != expected_size {
        return Err(internal("image_publication_local_manifest_size"));
    }
    check_deadline(deadline)?;
    let file = File::open(path).map_err(|_| internal("image_publication_local_manifest"))?;
    let capacity = usize::try_from(expected_size)
        .map_err(|_| internal("image_publication_local_manifest_size"))?;
    let mut bytes = Vec::with_capacity(capacity);
    file.take(expected_size.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| internal("image_publication_local_manifest"))?;
    check_deadline(deadline)?;
    if bytes.len() as u64 != expected_size {
        return Err(internal("image_publication_local_manifest_size"));
    }
    Ok(bytes)
}

fn verify_blob(
    publication_dir: &Path,
    layout: &Path,
    docker_config: &Path,
    repository: &str,
    index: usize,
    descriptor: &Descriptor,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let local_path = local_blob_path(layout, &descriptor.digest)?;
    let metadata =
        fs::symlink_metadata(&local_path).map_err(|_| internal("image_publication_local_blob"))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() != descriptor.size
    {
        return Err(internal("image_publication_local_blob_size"));
    }
    let readback_path = publication_dir.join(format!("crane-readback-{index:08}.blob"));
    let mut readback = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&readback_path)
        .map_err(|_| internal("image_publication_readback_create"))?;
    let reference = format!("{repository}@{}", descriptor.digest);
    let args = vec!["blob".into(), reference.into()];
    let (output, _drained) = super::crane_file(
        docker_config,
        args,
        &mut readback,
        descriptor.size,
        deadline,
    )?;
    if !output.success || output.code != Some(0) || output.signal.is_some() {
        return Err(internal("image_publication_blob_process"));
    }
    let readback_identity = readback
        .metadata()
        .map_err(|_| internal("image_publication_blob_size"))?;
    if output.stdout_bytes != descriptor.size || readback_identity.len() != descriptor.size {
        return Err(internal("image_publication_blob_size"));
    }
    compare_blob(
        &local_path,
        &mut readback,
        &descriptor.digest,
        descriptor.size,
        deadline,
    )?;
    drop(readback);
    remove_verified_readback(&readback_path, &readback_identity)
}

fn remove_verified_readback(
    path: &Path,
    created: &fs::Metadata,
) -> Result<(), OrchestratorError> {
    let named = fs::symlink_metadata(path)
        .map_err(|_| internal("image_publication_readback_identity"))?;
    if !created.is_file()
        || !named.is_file()
        || named.file_type().is_symlink()
        || created.dev() != named.dev()
        || created.ino() != named.ino()
    {
        return Err(internal("image_publication_readback_identity"));
    }
    fs::remove_file(path).map_err(|_| internal("image_publication_readback_cleanup"))
}

fn compare_blob(
    local_path: &Path,
    readback: &mut File,
    expected_digest: &str,
    expected_size: u64,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let mut local = File::open(local_path).map_err(|_| internal("image_publication_local_blob"))?;
    readback
        .seek(SeekFrom::Start(0))
        .map_err(|_| internal("image_publication_readback"))?;
    let mut local_chunk = vec![0_u8; COMPARE_CHUNK_BYTES].into_boxed_slice();
    let mut remote_chunk = vec![0_u8; COMPARE_CHUNK_BYTES].into_boxed_slice();
    let mut hasher = Sha256::new();
    let mut remaining = expected_size;
    let chunk_capacity = u64::try_from(COMPARE_CHUNK_BYTES)
        .map_err(|_| internal("image_publication_blob_chunk_size"))?;
    while remaining != 0 {
        check_deadline(deadline)?;
        let size = usize::try_from(remaining.min(chunk_capacity))
            .map_err(|_| internal("image_publication_blob_chunk_size"))?;
        local
            .read_exact(&mut local_chunk[..size])
            .map_err(|_| internal("image_publication_local_blob_read"))?;
        readback
            .read_exact(&mut remote_chunk[..size])
            .map_err(|_| internal("image_publication_readback_read"))?;
        if local_chunk[..size] != remote_chunk[..size] {
            return Err(internal("image_publication_blob_mismatch"));
        }
        hasher.update(&local_chunk[..size]);
        remaining = remaining
            .checked_sub(
                u64::try_from(size).map_err(|_| internal("image_publication_blob_chunk_size"))?,
            )
            .ok_or_else(|| internal("image_publication_blob_chunk_size"))?;
    }
    check_deadline(deadline)?;
    let digest = format!("sha256:{}", super::hex(&hasher.finalize()));
    if digest != expected_digest {
        return Err(internal("image_publication_local_blob_digest"));
    }
    Ok(())
}

fn local_blob_path(layout: &Path, digest: &str) -> Result<PathBuf, OrchestratorError> {
    let hex = digest
        .strip_prefix("sha256:")
        .filter(|value| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        .ok_or_else(|| internal("image_publication_blob_digest"))?;
    Ok(layout.join("blobs").join("sha256").join(hex))
}

fn check_deadline(deadline: CheckDeadline) -> Result<(), OrchestratorError> {
    deadline
        .remaining()
        .map(|_| ())
        .map_err(|_| internal("image_publication_deadline"))
}
