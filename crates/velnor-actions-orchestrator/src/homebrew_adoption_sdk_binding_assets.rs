// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Write},
    path::Path,
};

use sha2::{Digest, Sha256};
use velnor_actions_contract::{ReleaseManifest, ReleaseTarget};
use velnor_actions_mise::CheckDeadline;

use super::{
    MAX_ATTESTATION_BYTES, MAX_BINARY_BYTES, MAX_CALLEE_BYTES, MAX_MANIFEST_BYTES,
    MAX_PROVENANCE_BYTES, MAX_SIDECAR_BYTES, SDK_OWNER_ID, SDK_REPOSITORY, SDK_REPOSITORY_ID,
};
use crate::package_release_publish::{
    PublicReleaseAsset, PublicReleaseSnapshot, read_public_release_snapshot,
    verify_public_sdk_release_producer,
};
use crate::package_release_signer::verify_published_manifest_attestation;
use crate::{OrchestratorError, internal::internal};

#[path = "homebrew_adoption_sdk_binding_assets_temp.rs"]
mod temp;

pub(super) struct SdkReleaseCandidate {
    pub(super) snapshot: PublicReleaseSnapshot,
    pub(super) manifest: ReleaseManifest,
    pub(super) manifest_bytes: Vec<u8>,
    pub(super) host_target: ReleaseTarget,
    pub(super) host_binary_sha256: String,
    pub(super) seed_runner_target: &'static str,
    pub(super) seed_runner_binary_sha256: String,
}

pub(super) fn load_sdk_release_candidate(
    version: &str,
    source_sha: &str,
    manifest_sha256: &str,
    deadline: CheckDeadline,
) -> Result<SdkReleaseCandidate, OrchestratorError> {
    let tag = format!("v{version}");
    let snapshot = read_public_release_snapshot(
        SDK_REPOSITORY,
        SDK_REPOSITORY_ID,
        "main",
        &tag,
        &sdk_asset_size_limits(version),
        deadline,
    )?;
    if snapshot.draft || snapshot.prerelease || !snapshot.immutable || snapshot.tag != tag {
        return Err(internal("sdk_release_not_immutable"));
    }
    let manifest_asset = find_asset(&snapshot, "release-manifest.json")?;
    if manifest_asset.size > MAX_MANIFEST_BYTES as u64
        || manifest_asset.sha256 != manifest_sha256
        || manifest_asset.api_digest.as_deref() != Some(&format!("sha256:{manifest_sha256}"))
    {
        return Err(internal("sdk_release_manifest_digest_mismatch"));
    }
    let manifest_bytes = manifest_asset.bytes.clone();
    let manifest_text =
        std::str::from_utf8(&manifest_bytes).map_err(|_| internal("sdk_release_manifest_utf8"))?;
    let manifest = ReleaseManifest::parse_json_with_limit(
        manifest_text,
        "release-manifest.json",
        MAX_MANIFEST_BYTES,
    )
    .map_err(|_| internal("sdk_release_manifest_invalid"))?;
    manifest
        .validate("release-manifest.json")
        .map_err(|_| internal("sdk_release_manifest_invalid"))?;
    validate_manifest_identity(&manifest, version, source_sha, &snapshot.tag_source_sha)?;
    let host_target = current_release_target()?;
    let (host_binary_sha256, seed_runner_binary_sha256) =
        verify_binary_inventory(&snapshot, &manifest, version, host_target, deadline)?;
    Ok(SdkReleaseCandidate {
        snapshot,
        manifest,
        manifest_bytes,
        host_target,
        host_binary_sha256,
        seed_runner_target: ReleaseTarget::LinuxX86_64.triple(),
        seed_runner_binary_sha256,
    })
}

fn verify_binary_inventory(
    snapshot: &PublicReleaseSnapshot,
    manifest: &ReleaseManifest,
    version: &str,
    host_target: ReleaseTarget,
    deadline: CheckDeadline,
) -> Result<(String, String), OrchestratorError> {
    let (host_digest, seed_runner_digest) = target_binary_digests(manifest, host_target)?;
    for target in ReleaseTarget::ALL {
        deadline
            .remaining()
            .map_err(|_| internal("package_public_verification_deadline"))?;
        let record = manifest
            .record_for_target(target.triple())
            .ok_or_else(|| internal("sdk_release_manifest_target_missing"))?;
        let binary_name = binary_asset_name(version, target);
        let binary = find_asset(snapshot, &binary_name)?;
        if binary.size > MAX_BINARY_BYTES
            || binary.sha256 != record.sha256
            || binary.api_digest.as_deref() != Some(&format!("sha256:{}", record.sha256))
        {
            return Err(internal("sdk_release_binary_manifest_mismatch"));
        }
        verify_sidecar(
            find_asset(snapshot, &format!("{binary_name}.sha256"))?,
            &binary_name,
            &record.sha256,
        )?;
    }
    if host_digest != sha256_current_executable(deadline)? {
        return Err(internal("sdk_running_executable_manifest_mismatch"));
    }
    Ok((host_digest, seed_runner_digest))
}

pub(super) fn target_binary_digests(
    manifest: &ReleaseManifest,
    host_target: ReleaseTarget,
) -> Result<(String, String), OrchestratorError> {
    let host_digest = manifest
        .record_for_target(host_target.triple())
        .ok_or_else(|| internal("sdk_release_host_target_missing"))?
        .sha256
        .clone();
    let seed_runner_digest = manifest
        .record_for_target(ReleaseTarget::LinuxX86_64.triple())
        .ok_or_else(|| internal("sdk_release_seed_runner_target_missing"))?
        .sha256
        .clone();
    Ok((host_digest, seed_runner_digest))
}

pub(super) fn verify_sdk_release_attestation(
    candidate: &SdkReleaseCandidate,
    source_sha: &str,
    manifest_sha256: &str,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let attestation_dir = temp::private_attestation_dir()?;
    let manifest_path = write_private_temp_asset(
        attestation_dir.path(),
        "release-manifest.json",
        &candidate.manifest_bytes,
    )?;
    let bundle = find_asset(&candidate.snapshot, "release-manifest.json.intoto.jsonl")?;
    let bundle_path = write_private_temp_asset(
        attestation_dir.path(),
        "release-manifest.json.intoto.jsonl",
        &bundle.bytes,
    )?;
    let attestation = verify_published_manifest_attestation(
        &manifest_path,
        &bundle_path,
        &crate::package_release_signer::SdkManifestAttestationExpectation {
            repository: SDK_REPOSITORY,
            repository_id: SDK_REPOSITORY_ID,
            owner_id: SDK_OWNER_ID,
            source_sha,
            manifest_sha256,
        },
        deadline,
    )?;
    if attestation.source_sha != source_sha
        || attestation.workflow_sha != source_sha
        || attestation.event != "workflow_dispatch"
    {
        return Err(internal("sdk_manifest_attestation_binding_mismatch"));
    }
    verify_public_sdk_release_producer(
        &crate::package_release_publish::PublicSdkProducerSelection {
            repository: SDK_REPOSITORY.to_owned(),
            repository_id: SDK_REPOSITORY_ID,
            source_sha: source_sha.to_owned(),
            run_id: attestation.run_id,
            run_attempt: attestation.run_attempt,
        },
        deadline,
    )
}

pub(super) fn verify_published_adoption_callee(
    version: &str,
    source_sha: &str,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let pins = crate::product_release_pins::homebrew_adoption_pins()?;
    let rendered =
        velnor_actions_workflow_renderer::render_homebrew_adoption_callee(version, &pins)
            .map_err(|_| internal("sdk_adoption_callee_template_invalid"))?;
    let source = crate::package_release_publish::read_protected_source_file(
        SDK_REPOSITORY,
        SDK_REPOSITORY_ID,
        "main",
        source_sha,
        ".github/workflows/homebrew-adoption.yml",
        MAX_CALLEE_BYTES,
        deadline,
    )?;
    if rendered.file.path != ".github/workflows/homebrew-adoption.yml"
        || source != rendered.file.bytes.as_bytes()
    {
        return Err(internal("sdk_adoption_callee_template_mismatch"));
    }
    Ok(())
}

pub(super) fn sdk_asset_size_limits(version: &str) -> BTreeMap<String, u64> {
    let mut limits = BTreeMap::new();
    for target in ReleaseTarget::ALL {
        let binary = binary_asset_name(version, target);
        limits.insert(binary.clone(), MAX_BINARY_BYTES);
        limits.insert(format!("{binary}.sha256"), MAX_SIDECAR_BYTES);
        limits.insert(format!("{binary}.provenance.json"), MAX_PROVENANCE_BYTES);
        for name in [
            binary.clone(),
            format!("{binary}.sha256"),
            format!("{binary}.provenance.json"),
        ] {
            limits.insert(format!("{name}.intoto.jsonl"), MAX_ATTESTATION_BYTES);
        }
    }
    limits.insert(
        "release-manifest.json".to_owned(),
        MAX_MANIFEST_BYTES as u64,
    );
    limits.insert(
        "release-manifest.json.intoto.jsonl".to_owned(),
        MAX_ATTESTATION_BYTES,
    );
    limits
}

pub(super) fn binary_asset_name(version: &str, target: ReleaseTarget) -> String {
    format!("velnor-actions-{version}-{}", target.triple())
}

pub(super) fn validate_manifest_identity(
    manifest: &ReleaseManifest,
    version: &str,
    source_sha: &str,
    tag_source_sha: &str,
) -> Result<(), OrchestratorError> {
    if manifest.repository != SDK_REPOSITORY
        || manifest.version != version
        || manifest.commit != source_sha
        || tag_source_sha != source_sha
    {
        return Err(internal("sdk_release_manifest_identity_mismatch"));
    }
    Ok(())
}

pub(super) fn find_asset<'a>(
    snapshot: &'a PublicReleaseSnapshot,
    name: &str,
) -> Result<&'a PublicReleaseAsset, OrchestratorError> {
    snapshot
        .assets
        .iter()
        .find(|asset| asset.name == name)
        .ok_or_else(|| internal("sdk_release_asset_missing"))
}

pub(super) fn verify_sidecar(
    asset: &PublicReleaseAsset,
    binary_name: &str,
    expected_sha256: &str,
) -> Result<(), OrchestratorError> {
    if asset.size > MAX_SIDECAR_BYTES
        || asset.sha256 != sha256(&asset.bytes)
        || asset.api_digest.as_deref() != Some(&format!("sha256:{}", asset.sha256))
    {
        return Err(internal("sdk_release_sidecar_asset_invalid"));
    }
    let text =
        std::str::from_utf8(&asset.bytes).map_err(|_| internal("sdk_release_sidecar_invalid"))?;
    let mut lines = text.lines();
    let line = lines
        .next()
        .ok_or_else(|| internal("sdk_release_sidecar_invalid"))?;
    let fields = line.split_ascii_whitespace().collect::<Vec<_>>();
    if lines.next().is_some()
        || fields.len() != 2
        || fields[0] != expected_sha256
        || fields[1].trim_start_matches('*') != binary_name
    {
        return Err(internal("sdk_release_sidecar_mismatch"));
    }
    Ok(())
}

pub(super) fn current_release_target() -> Result<ReleaseTarget, OrchestratorError> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Ok(ReleaseTarget::LinuxX86_64),
        ("macos", "aarch64") => Ok(ReleaseTarget::MacosArm64),
        ("macos", "x86_64") => Ok(ReleaseTarget::MacosX86_64),
        _ => Err(internal("sdk_host_target_unsupported")),
    }
}

pub(super) fn sha256_current_executable(
    deadline: CheckDeadline,
) -> Result<String, OrchestratorError> {
    let executable = std::env::current_exe()
        .map_err(|error| OrchestratorError::io("current_exe", error.to_string()))?;
    let canonical = std::fs::canonicalize(&executable)
        .map_err(|error| OrchestratorError::io("current_exe", error.to_string()))?;
    let mut file = File::open(&canonical).map_err(|error| {
        OrchestratorError::io(canonical.display().to_string(), error.to_string())
    })?;
    let metadata = file.metadata().map_err(|error| {
        OrchestratorError::io(canonical.display().to_string(), error.to_string())
    })?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_BINARY_BYTES {
        return Err(internal("sdk_running_executable_invalid"));
    }
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        deadline
            .remaining()
            .map_err(|_| internal("package_public_verification_deadline"))?;
        let count = file.read(&mut buffer).map_err(|error| {
            OrchestratorError::io(canonical.display().to_string(), error.to_string())
        })?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(count as u64)
            .filter(|value| *value <= MAX_BINARY_BYTES)
            .ok_or_else(|| internal("sdk_running_executable_invalid"))?;
        hasher.update(&buffer[..count]);
    }
    Ok(hex(&hasher.finalize()))
}

pub(super) fn write_private_temp_asset(
    directory: &Path,
    name: &str,
    bytes: &[u8],
) -> Result<std::path::PathBuf, OrchestratorError> {
    if !matches!(
        name,
        "release-manifest.json" | "release-manifest.json.intoto.jsonl"
    ) {
        return Err(internal("sdk_asset_temp_name_invalid"));
    }
    let path = directory.join(name);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options
        .open(&path)
        .map_err(|error| OrchestratorError::io(path.display().to_string(), error.to_string()))?;
    file.write_all(bytes)
        .map_err(|error| OrchestratorError::io(path.display().to_string(), error.to_string()))?;
    Ok(path)
}

pub(super) fn compare_stable_versions(
    left: &str,
    right: &str,
) -> Result<std::cmp::Ordering, OrchestratorError> {
    let parse = |version: &str| -> Result<(u64, u64, u64), OrchestratorError> {
        let mut parts = version.split('.').map(str::parse::<u64>);
        match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(Ok(a)), Some(Ok(b)), Some(Ok(c)), None) => Ok((a, b, c)),
            _ => Err(internal("sdk_version_invalid")),
        }
    };
    Ok(parse(left)?.cmp(&parse(right)?))
}

pub(super) fn sha256(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

pub(super) fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
        output.push(char::from(b"0123456789abcdef"[usize::from(byte & 0x0f)]));
    }
    output
}
