//! Read-only verification of local generator binaries against a release manifest.
//!
//! This binds declared manifest digests and source SHA to caller-supplied
//! expectations. It does not authenticate those expectations or establish
//! publication, artifact-run, ABI, or attestation provenance. File bytes are
//! observed through opened handles; concurrent writes and ancestor symlinks
//! are outside this local verification boundary.

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

/// Verify a release manifest against two local target binaries.
///
/// `expected_source_commit` is a caller-provided expectation, not authenticated
/// provenance. The function performs only bounded local reads: it does not
/// execute binaries, inspect sidecars, contact a release service, or write.
/// A final-component symlink and non-regular file are refused. Ancestor-link
/// resolution and concurrent mutation of an already opened file are not ruled
/// out, so this is a local byte check rather than a race-free security proof.
///
/// # Errors
///
/// Returns an error for malformed or mismatched manifest identity, unreadable
/// inputs, unsupported file types, empty/oversize binaries, or digest mismatch.
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

#[cfg(test)]
#[path = "local_release_manifest_tests.rs"]
mod tests;
