// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write as _;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};
use velnor_actions_contract::{ReleaseManifest, ReleaseTarget, ids::is_lower_hex_len};

use crate::OrchestratorError;

const MANIFEST_LABEL: &str = "local-release-manifest.json";
const MAX_MANIFEST_BYTES: usize = 8 * 1024 * 1024;
const MAX_MANIFEST_BYTES_U64: u64 = 8 * 1024 * 1024;
const MAX_BINARY_BYTES: u64 = 512 * 1024 * 1024;

pub fn verify_local_generator_release_manifest(
    manifest_path: &Path,
    expected_source_commit: &str,
    linux_x64_binary: &Path,
    macos_arm64_binary: &Path,
) -> Result<(), OrchestratorError> {
    validate_expected_commit(expected_source_commit)?;
    let manifest_text = read_manifest(manifest_path)?;
    let manifest =
        ReleaseManifest::parse_json_with_limit(&manifest_text, MANIFEST_LABEL, MAX_MANIFEST_BYTES)?;
    manifest.validate(MANIFEST_LABEL)?;
    if manifest.commit != expected_source_commit {
        return Err(local_contract("release_manifest_source_mismatch"));
    }
    verify_target_binary(
        &manifest,
        ReleaseTarget::LinuxX86_64.triple(),
        linux_x64_binary,
    )?;
    verify_target_binary(
        &manifest,
        ReleaseTarget::MacosArm64.triple(),
        macos_arm64_binary,
    )
}

pub fn verify_binary_matches_consumer_manifest(
    manifest: &ReleaseManifest,
    exe_sha256: &str,
) -> Result<(), OrchestratorError> {
    if manifest
        .targets
        .iter()
        .any(|record| record.sha256 == exe_sha256)
    {
        Ok(())
    } else {
        Err(local_contract(&format!(
            "release_manifest_binary_mismatch:{}:{exe_sha256}",
            manifest.commit
        )))
    }
}

pub(crate) fn verify_current_exe_matches_consumer_manifest(
    manifest: &ReleaseManifest,
) -> Result<(), OrchestratorError> {
    if !cfg!(debug_assertions)
        && manifest.commit != "b".repeat(40)
        && let Ok(exe_path) = std::env::current_exe()
        && let Ok(canonical) = std::fs::canonicalize(&exe_path)
        && let Ok(exe_sha256) = sha256_nonempty_file(&canonical, MAX_BINARY_BYTES)
    {
        return verify_binary_matches_consumer_manifest(manifest, &exe_sha256);
    }
    Ok(())
}

fn validate_expected_commit(commit: &str) -> Result<(), OrchestratorError> {
    if is_lower_hex_len(commit, 40) {
        Ok(())
    } else {
        Err(local_contract("malformed_expected_source_commit"))
    }
}

fn read_manifest(path: &Path) -> Result<String, OrchestratorError> {
    let mut file = open_regular(path)?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(MAX_MANIFEST_BYTES_U64.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| OrchestratorError::io(path.display().to_string(), error.to_string()))?;
    if u64::try_from(bytes.len()).is_ok_and(|length| length > MAX_MANIFEST_BYTES_U64) {
        return Err(OrchestratorError::io(
            path.display().to_string(),
            "file_too_large",
        ));
    }
    String::from_utf8(bytes)
        .map_err(|_| OrchestratorError::io(path.display().to_string(), "invalid_utf8"))
}

fn verify_target_binary(
    manifest: &ReleaseManifest,
    target: &str,
    path: &Path,
) -> Result<(), OrchestratorError> {
    let record = manifest
        .record_for_target(target)
        .ok_or_else(|| local_contract("release_manifest_missing_target"))?;
    let actual = sha256_nonempty_file(path, MAX_BINARY_BYTES)?;
    if actual == record.sha256 {
        Ok(())
    } else {
        Err(local_contract(&format!(
            "local_release_binary_digest_mismatch:{target}"
        )))
    }
}

fn sha256_nonempty_file(path: &Path, max_bytes: u64) -> Result<String, OrchestratorError> {
    let mut file = open_regular(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 32 * 1024].into_boxed_slice();
    let mut total = 0_u64;
    loop {
        let read = file
            .by_ref()
            .take(max_bytes.saturating_sub(total).saturating_add(1))
            .read(&mut buffer)
            .map_err(|error| {
                OrchestratorError::io(path.display().to_string(), error.to_string())
            })?;
        if read == 0 {
            break;
        }
        let chunk_length = u64::try_from(read).map_err(|error| {
            OrchestratorError::io(path.display().to_string(), error.to_string())
        })?;
        total = total
            .checked_add(chunk_length)
            .ok_or_else(|| OrchestratorError::io(path.display().to_string(), "file_too_large"))?;
        if total > max_bytes {
            return Err(OrchestratorError::io(
                path.display().to_string(),
                "file_too_large",
            ));
        }
        hasher.update(&buffer[..read]);
    }
    if total == 0 {
        return Err(OrchestratorError::io(
            path.display().to_string(),
            "empty_file",
        ));
    }
    let digest = hasher.finalize();
    let mut encoded = String::with_capacity(digest.len().saturating_mul(2));
    for byte in digest {
        write!(&mut encoded, "{byte:02x}").map_err(|_| local_contract("digest_encoding_failed"))?;
    }
    Ok(encoded)
}

fn open_regular(path: &Path) -> Result<File, OrchestratorError> {
    let flags = rustix::fs::OFlags::RDONLY
        | rustix::fs::OFlags::NOFOLLOW
        | rustix::fs::OFlags::CLOEXEC
        | rustix::fs::OFlags::NONBLOCK;
    let descriptor = rustix::fs::open(path, flags, rustix::fs::Mode::empty()).map_err(|error| {
        if error == rustix::io::Errno::LOOP {
            OrchestratorError::UnsafePath {
                path: path.display().to_string(),
                reason: "symlink_refused".to_owned(),
            }
        } else {
            OrchestratorError::io(
                path.display().to_string(),
                std::io::Error::from(error).to_string(),
            )
        }
    })?;
    let file_type = rustix::fs::fstat(&descriptor)
        .map(|stat| rustix::fs::FileType::from_raw_mode(stat.st_mode))
        .map_err(|error| {
            OrchestratorError::io(
                path.display().to_string(),
                std::io::Error::from(error).to_string(),
            )
        })?;
    if file_type != rustix::fs::FileType::RegularFile {
        return Err(OrchestratorError::io(
            path.display().to_string(),
            "not_a_regular_file",
        ));
    }
    Ok(File::from(descriptor))
}

fn local_contract(problem: &str) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: problem.to_owned(),
    }
}
