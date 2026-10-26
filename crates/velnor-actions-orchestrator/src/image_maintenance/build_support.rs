// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    fs::{self, File},
    io::Read,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

use crate::{OrchestratorError, internal::internal};

pub(super) const MAX_BUILDX_BYTES: u64 = 100_000_000;
pub(super) const DOCKER_ENV: &str = "docker-config";

pub(super) fn verify_buildx_url(url: &str, version: &str) -> Result<(), OrchestratorError> {
    if url
        != format!(
            "https://github.com/docker/buildx/releases/download/v{version}/buildx-v{version}.linux-amd64"
        )
    {
        return Err(internal("image_candidate_buildx_asset"));
    }
    Ok(())
}

pub(super) fn owned_directory(path: &Path) -> Result<PathBuf, OrchestratorError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| OrchestratorError::io(path.display().to_string(), error.to_string()))?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != crate::filesystem_owner::effective_uid()
    {
        return Err(internal("image_candidate_temp_directory"));
    }
    path.canonicalize()
        .map_err(|error| OrchestratorError::io(path.display().to_string(), error.to_string()))
}

pub(in crate::image_maintenance) fn private_subdirectory(
    root: &Path,
    relative: &str,
) -> Result<PathBuf, OrchestratorError> {
    let mut path = root.to_path_buf();
    for component in relative.split('/') {
        path.push(component);
        match fs::create_dir(&path) {
            Ok(()) => {
                fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).map_err(|error| {
                    OrchestratorError::io(path.display().to_string(), error.to_string())
                })?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let metadata = fs::symlink_metadata(&path).map_err(|error| {
                    OrchestratorError::io(path.display().to_string(), error.to_string())
                })?;
                if !metadata.is_dir()
                    || metadata.file_type().is_symlink()
                    || metadata.uid() != crate::filesystem_owner::effective_uid()
                    || metadata.mode() & 0o077 != 0
                {
                    return Err(internal("image_candidate_private_directory"));
                }
            }
            Err(error) => {
                return Err(OrchestratorError::io(
                    path.display().to_string(),
                    error.to_string(),
                ));
            }
        }
    }
    Ok(path)
}

pub(super) fn hash_regular(path: &Path, limit: u64) -> Result<String, OrchestratorError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| OrchestratorError::io(path.display().to_string(), error.to_string()))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > limit
    {
        return Err(internal("image_candidate_regular_file"));
    }
    let fd = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::CLOEXEC
            | rustix::fs::OFlags::NONBLOCK,
        rustix::fs::Mode::empty(),
    )
    .map_err(|_| internal("image_candidate_hash_open"))?;
    let mut file = File::from(fd);
    let opened = file
        .metadata()
        .map_err(|_| internal("image_candidate_hash_stat"))?;
    if opened.dev() != metadata.dev() || opened.ino() != metadata.ino() {
        return Err(internal("image_candidate_hash_identity"));
    }
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 128 * 1024];
    let mut size = 0_u64;
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| internal("image_candidate_hash_read"))?;
        if count == 0 {
            break;
        }
        size = size
            .checked_add(count as u64)
            .filter(|size| *size <= limit)
            .ok_or_else(|| internal("image_candidate_hash_limit"))?;
        hasher.update(&buffer[..count]);
    }
    let after = file
        .metadata()
        .map_err(|_| internal("image_candidate_hash_stat"))?;
    let path_after =
        fs::symlink_metadata(path).map_err(|_| internal("image_candidate_hash_stat"))?;
    if size != metadata.len()
        || after.dev() != opened.dev()
        || after.ino() != opened.ino()
        || after.len() != metadata.len()
        || after.mtime() != opened.mtime()
        || after.mtime_nsec() != opened.mtime_nsec()
        || after.ctime() != opened.ctime()
        || after.ctime_nsec() != opened.ctime_nsec()
        || path_after.dev() != metadata.dev()
        || path_after.ino() != metadata.ino()
        || !path_after.is_file()
        || path_after.file_type().is_symlink()
    {
        return Err(internal("image_candidate_hash_changed"));
    }
    Ok(hex(&hasher.finalize()))
}

pub(in crate::image_maintenance) fn install_verified_buildx(
    staged: &Path,
    plugin: &Path,
    expected_sha256: &str,
) -> Result<(), OrchestratorError> {
    if hash_regular(staged, MAX_BUILDX_BYTES)? != expected_sha256 {
        return Err(internal(
            "image_candidate_buildx_checksum_differs_from_.velnor/runner.lock",
        ));
    }
    fs::set_permissions(staged, fs::Permissions::from_mode(0o755))
        .map_err(|error| OrchestratorError::io(staged.display().to_string(), error.to_string()))?;
    fs::hard_link(staged, plugin)
        .map_err(|error| OrchestratorError::io(plugin.display().to_string(), error.to_string()))?;
    let expected =
        fs::metadata(staged).map_err(|_| internal("image_candidate_buildx_staged_stat"))?;
    let installed = fs::symlink_metadata(plugin)
        .map_err(|_| internal("image_candidate_buildx_installed_stat"))?;
    if expected.dev() != installed.dev()
        || expected.ino() != installed.ino()
        || installed.mode() & 0o777 != 0o755
        || hash_regular(plugin, MAX_BUILDX_BYTES)? != expected_sha256
    {
        return Err(internal("image_candidate_buildx_publish"));
    }
    Ok(())
}

pub(super) fn path_exists(path: &Path) -> Result<bool, OrchestratorError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(OrchestratorError::io(
            path.display().to_string(),
            error.to_string(),
        )),
    }
}

pub(super) fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 15)]));
    }
    output
}
