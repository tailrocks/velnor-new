//! Strict handoff, directory, and byte inventory admission.

use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
    time::{Duration, Instant},
};

use sha2::{Digest, Sha256};

use super::{
    ArtifactKind, ExpectedFile, MAX_ARTIFACT_BYTES, MAX_HANDOFF_BYTES, MAX_PATH_BYTES,
    MAX_PAYLOAD_BYTES, MAX_SBOM_BYTES, MAX_TOTAL_BYTES, PackageOutputCompanion,
    PackageOutputManifest, PackageOutputPayload, PackageOutputSbom, VerifiedPackageArtifact,
    invalid,
};
use crate::OrchestratorError;
use velnor_actions_contract::package_release::{PackageReleaseAsset, PackageReleaseAssetKind};

#[path = "package_release_artifacts_tree.rs"]
mod tree;
pub(crate) use tree::validate_source_tree;

/// Compare every untrusted handoff row to the exact trusted inventory.
pub(super) fn compare_manifest_inventory(
    manifest: &PackageOutputManifest,
    expected: &[ExpectedFile],
) -> Result<(), OrchestratorError> {
    let payloads = expected
        .iter()
        .filter(|file| file.kind == ArtifactKind::Payload)
        .collect::<Vec<_>>();
    let companions = expected
        .iter()
        .filter(|file| file.kind == ArtifactKind::Companion)
        .collect::<Vec<_>>();
    let sboms = expected
        .iter()
        .filter(|file| file.kind == ArtifactKind::Sbom)
        .collect::<Vec<_>>();
    if manifest.payloads.len() != payloads.len()
        || manifest.companions.len() != companions.len()
        || manifest.sboms.len() != sboms.len()
    {
        return Err(invalid("handoff_inventory_count_mismatch"));
    }
    compare_payload_inventory(&manifest.payloads, &payloads)?;
    compare_companion_inventory(&manifest.companions, &companions)?;
    compare_sbom_inventory(&manifest.sboms, &sboms)
}

fn compare_payload_inventory(
    actual_rows: &[PackageOutputPayload],
    expected_rows: &[&ExpectedFile],
) -> Result<(), OrchestratorError> {
    let mut expected_payloads = expected_rows
        .iter()
        .map(|file| {
            Ok((
                file.name.as_str(),
                file.product_id.as_deref().unwrap_or_default(),
                file.target.as_deref().unwrap_or_default(),
                file.payload_format
                    .ok_or_else(|| invalid("expected_payload_format_missing"))?,
            ))
        })
        .collect::<Result<Vec<_>, OrchestratorError>>()?;
    expected_payloads.sort_unstable();
    let mut actual_payloads = actual_rows
        .iter()
        .map(|row| {
            (
                row.name.as_str(),
                row.product_id.as_str(),
                row.target.as_str(),
                row.format,
            )
        })
        .collect::<Vec<_>>();
    actual_payloads.sort_unstable();
    if actual_payloads != expected_payloads
        || actual_rows.iter().any(|row| !lower_sha256(&row.sha256))
    {
        return Err(invalid("handoff_payload_inventory_mismatch"));
    }
    Ok(())
}

fn compare_companion_inventory(
    actual_rows: &[PackageOutputCompanion],
    expected_rows: &[&ExpectedFile],
) -> Result<(), OrchestratorError> {
    let mut expected_companions = expected_rows
        .iter()
        .map(|file| {
            (
                file.companion_id.as_deref().unwrap_or_default(),
                file.path.as_str(),
                file.name.as_str(),
            )
        })
        .collect::<Vec<_>>();
    expected_companions.sort_unstable();
    let mut actual_companions = actual_rows
        .iter()
        .map(|row| (row.id.as_str(), row.path.as_str(), row.name.as_str()))
        .collect::<Vec<_>>();
    actual_companions.sort_unstable();
    if actual_companions != expected_companions
        || actual_rows.iter().any(|row| !lower_sha256(&row.sha256))
    {
        return Err(invalid("handoff_companion_inventory_mismatch"));
    }
    Ok(())
}

fn compare_sbom_inventory(
    actual_rows: &[PackageOutputSbom],
    expected_rows: &[&ExpectedFile],
) -> Result<(), OrchestratorError> {
    let mut expected_sboms = expected_rows
        .iter()
        .map(|file| {
            (
                file.archive_subject.as_deref().unwrap_or_default(),
                file.name.as_str(),
            )
        })
        .collect::<Vec<_>>();
    expected_sboms.sort_unstable();
    let mut actual_sboms = actual_rows
        .iter()
        .map(|row| (row.payload_name.as_str(), row.name.as_str()))
        .collect::<Vec<_>>();
    actual_sboms.sort_unstable();
    if actual_sboms != expected_sboms || actual_rows.iter().any(|row| !lower_sha256(&row.sha256)) {
        return Err(invalid("handoff_sbom_inventory_mismatch"));
    }
    Ok(())
}

/// Create a new empty staging directory without following the leaf path.
pub(super) fn create_fresh_staging(path: &Path) -> Result<(), OrchestratorError> {
    fs::create_dir(path)
        .map_err(|error| OrchestratorError::io(path.display().to_string(), error.to_string()))
}

/// Copy and hash every declared asset into Velnor-owned staging.
pub(super) fn copy_expected_files(
    producer: &Path,
    staging: &Path,
    expected: &[ExpectedFile],
    manifest: &PackageOutputManifest,
    started: Instant,
) -> Result<Vec<VerifiedPackageArtifact>, OrchestratorError> {
    let claimed = claimed_digests(manifest);
    let mut total = 0u64;
    let mut output = Vec::new();
    for file in expected {
        if file.kind == ArtifactKind::Handoff {
            continue;
        }
        check_deadline(started)?;
        let source = producer.join(&file.path);
        let name_path = Path::new(&file.name);
        if name_path.components().count() != 1 || file.name.len() > MAX_PATH_BYTES {
            return Err(invalid("release_asset_name_invalid"));
        }
        let destination = staging.join(name_path);
        let max = match file.kind {
            ArtifactKind::Payload => MAX_PAYLOAD_BYTES,
            ArtifactKind::Sbom => MAX_SBOM_BYTES,
            ArtifactKind::Companion => MAX_ARTIFACT_BYTES,
            ArtifactKind::Handoff => MAX_HANDOFF_BYTES,
        };
        let (size, sha256) = copy_hash_file(&source, &destination, max, &mut total, started)?;
        if claimed.get(file.path.as_str()).copied() != Some(sha256.as_str()) {
            return Err(invalid("producer_claimed_digest_mismatch"));
        }
        output.push(VerifiedPackageArtifact {
            name: file.name.clone(),
            path: destination,
            size,
            sha256: sha256.clone(),
            release_asset: PackageReleaseAsset {
                name: file.name.clone(),
                kind: match file.kind {
                    ArtifactKind::Payload => PackageReleaseAssetKind::Payload,
                    ArtifactKind::Companion => PackageReleaseAssetKind::Companion,
                    ArtifactKind::Sbom => PackageReleaseAssetKind::Sbom,
                    ArtifactKind::Handoff => return Err(invalid("handoff_asset_unexpected")),
                },
                product_id: file.product_id.clone(),
                target: file.target.clone(),
                subject_name: file.archive_subject.clone(),
                size,
                sha256,
            },
        });
    }
    output.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(output)
}

fn claimed_digests(manifest: &PackageOutputManifest) -> BTreeMap<&str, &str> {
    manifest
        .payloads
        .iter()
        .map(|row| (row.name.as_str(), row.sha256.as_str()))
        .chain(
            manifest
                .companions
                .iter()
                .map(|row| (row.path.as_str(), row.sha256.as_str())),
        )
        .chain(
            manifest
                .sboms
                .iter()
                .map(|row| (row.name.as_str(), row.sha256.as_str())),
        )
        .collect()
}

fn copy_hash_file(
    source: &Path,
    destination: &Path,
    max_size: u64,
    total: &mut u64,
    started: Instant,
) -> Result<(u64, String), OrchestratorError> {
    let mut input = open_regular_nofollow(source)?;
    let size = input
        .metadata()
        .map_err(|error| OrchestratorError::io(source.display().to_string(), error.to_string()))?
        .len();
    if size == 0 || size > max_size || total.saturating_add(size) > MAX_TOTAL_BYTES {
        return Err(invalid("producer_output_size_limit"));
    }
    let mut output = create_private_file(destination)?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024].into_boxed_slice();
    let mut copied = 0u64;
    loop {
        check_deadline(started)?;
        let read = input.read(&mut buffer).map_err(|error| {
            OrchestratorError::io(source.display().to_string(), error.to_string())
        })?;
        if read == 0 {
            break;
        }
        copied = copied.saturating_add(read as u64);
        if copied > max_size || copied > size {
            return Err(invalid("producer_output_changed_during_read"));
        }
        hash.update(&buffer[..read]);
        output.write_all(&buffer[..read]).map_err(|error| {
            OrchestratorError::io(destination.display().to_string(), error.to_string())
        })?;
    }
    if copied != size {
        return Err(invalid("producer_output_changed_during_read"));
    }
    output.sync_all().map_err(|error| {
        OrchestratorError::io(destination.display().to_string(), error.to_string())
    })?;
    *total = total.saturating_add(copied);
    Ok((copied, hex_lower(&hash.finalize())))
}

/// Read one regular file without following its final symlink.
pub(super) fn open_regular_nofollow(path: &Path) -> Result<File, OrchestratorError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| OrchestratorError::io(path.display().to_string(), error.to_string()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(invalid("producer_output_not_regular_file"));
    }
    #[cfg(unix)]
    {
        let descriptor = rustix::fs::open(
            path,
            rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .map_err(|error| OrchestratorError::io(path.display().to_string(), error.to_string()))?;
        let stat = rustix::fs::fstat(&descriptor).map_err(|error| {
            OrchestratorError::io(path.display().to_string(), error.to_string())
        })?;
        if rustix::fs::FileType::from_raw_mode(stat.st_mode) != rustix::fs::FileType::RegularFile {
            return Err(invalid("producer_output_not_regular_file"));
        }
        Ok(File::from(descriptor))
    }
    #[cfg(not(unix))]
    {
        OpenOptions::new()
            .read(true)
            .open(path)
            .map_err(|error| OrchestratorError::io(path.display().to_string(), error.to_string()))
    }
}

fn create_private_file(path: &Path) -> Result<File, OrchestratorError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .map_err(|error| OrchestratorError::io(path.display().to_string(), error.to_string()))
}

/// Read a bounded file through a no-follow regular-file descriptor.
pub(super) fn read_limited(
    path: &Path,
    limit: u64,
    started: Instant,
) -> Result<Vec<u8>, OrchestratorError> {
    let mut file = open_regular_nofollow(path)?;
    let size = file
        .metadata()
        .map_err(|error| OrchestratorError::io(path.display().to_string(), error.to_string()))?
        .len();
    if size == 0 || size > limit {
        return Err(invalid("package_output_metadata_size_limit"));
    }
    let capacity = usize::try_from(size).map_err(|_| invalid("package_output_size_limit"))?;
    let mut bytes = Vec::with_capacity(capacity);
    let mut buffer = vec![0u8; 32 * 1024].into_boxed_slice();
    loop {
        check_deadline(started)?;
        let read = file.read(&mut buffer).map_err(|error| {
            OrchestratorError::io(path.display().to_string(), error.to_string())
        })?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..read]);
        if bytes.len() as u64 > limit || bytes.len() as u64 > size {
            return Err(invalid("package_output_metadata_changed_during_read"));
        }
    }
    if bytes.len() as u64 != size {
        return Err(invalid("package_output_metadata_changed_during_read"));
    }
    Ok(bytes)
}

pub(super) fn ensure_no_symlink_path(root: &Path, relative: &str) -> Result<(), OrchestratorError> {
    let root_meta = fs::symlink_metadata(root)
        .map_err(|error| OrchestratorError::io(root.display().to_string(), error.to_string()))?;
    if root_meta.file_type().is_symlink() || !root_meta.is_dir() {
        return Err(invalid("producer_output_root_not_regular_directory"));
    }
    let mut current = root.to_path_buf();
    for component in Path::new(relative).components() {
        let std::path::Component::Normal(part) = component else {
            return Err(invalid("package_output_path_invalid"));
        };
        current.push(part);
        let metadata = fs::symlink_metadata(&current).map_err(|error| {
            OrchestratorError::io(current.display().to_string(), error.to_string())
        })?;
        if metadata.file_type().is_symlink() {
            return Err(invalid("producer_output_symlink"));
        }
    }
    Ok(())
}

fn lower_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn check_deadline(started: Instant) -> Result<(), OrchestratorError> {
    if velnor_actions_mise::CheckDeadline::check_scope().is_err()
        || started.elapsed() > Duration::from_secs(120)
    {
        return Err(invalid("package_output_verification_timeout"));
    }
    Ok(())
}
