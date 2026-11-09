// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_mise::CheckDeadline;

use crate::{
    OrchestratorError, homebrew::adoption::ProtectedHomebrewAdoptionTransition, internal::internal,
};

const SDK_REPOSITORY: &str = "tailrocks/velnor-new";
const SDK_REPOSITORY_ID: u64 = 1_390_620_900;
const SDK_OWNER_ID: u64 = 78_806_509;
const SDK_CAPABILITY: &str = "homebrew-adoption-seed-v1";
const MAX_MANIFEST_BYTES: usize = 8 * 1024 * 1024;
const MAX_CALLEE_BYTES: usize =
    velnor_actions_contract::package_release_policy::MAX_CARGO_VERSION_MANIFEST_BYTES;
const MAX_BINARY_BYTES: u64 = 128 * 1024 * 1024;
const MAX_SIDECAR_BYTES: u64 = 1024;
const MAX_PROVENANCE_BYTES: u64 = 64 * 1024;
const MAX_ATTESTATION_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PublishedSdkBindingIdentity {
    pub(crate) repository: &'static str,
    pub(crate) source_sha: String,
    pub(crate) version: String,
    pub(crate) manifest_sha256: String,
    pub(crate) seed_runner_target: &'static str,
    pub(crate) seed_runner_binary_sha256: String,
    pub(crate) host_target: &'static str,
    pub(crate) host_binary_sha256: String,
}

#[derive(Debug, Clone)]
pub(crate) struct VerifiedPublishedSdkBinding {
    identity: PublishedSdkBindingIdentity,
    manifest_bytes: Vec<u8>,
}

impl VerifiedPublishedSdkBinding {
    pub(crate) fn identity(&self) -> &PublishedSdkBindingIdentity {
        &self.identity
    }

    pub(crate) fn manifest_bytes(&self) -> &[u8] {
        &self.manifest_bytes
    }
}

pub(crate) fn verify_published_sdk_binding(
    transition: &ProtectedHomebrewAdoptionTransition,
    version: &str,
    source_sha: &str,
    manifest_sha256: &str,
    deadline: CheckDeadline,
) -> Result<VerifiedPublishedSdkBinding, OrchestratorError> {
    let transition = transition.transition();
    validate_selection(transition, version, source_sha, manifest_sha256)?;
    deadline
        .remaining()
        .map_err(|_| internal("package_public_verification_deadline"))?;
    let candidate = load_sdk_release_candidate(version, source_sha, manifest_sha256, deadline)?;
    verify_sdk_release_attestation(&candidate, source_sha, manifest_sha256, deadline)?;
    verify_published_adoption_callee(version, source_sha, deadline)?;

    Ok(VerifiedPublishedSdkBinding {
        identity: PublishedSdkBindingIdentity {
            repository: SDK_REPOSITORY,
            source_sha: candidate.manifest.commit.clone(),
            version: candidate.manifest.version.clone(),
            manifest_sha256: manifest_sha256.to_owned(),
            seed_runner_target: candidate.seed_runner_target,
            seed_runner_binary_sha256: candidate.seed_runner_binary_sha256,
            host_target: candidate.host_target.triple(),
            host_binary_sha256: candidate.host_binary_sha256,
        },
        manifest_bytes: candidate.manifest_bytes,
    })
}

pub(crate) fn verify_sdk_for_mac(
    expected: &velnor_actions_contract::runner_image_manifest::mac_attestation::MacSdkIdentity,
    generator_lock: &velnor_actions_contract::GeneratorLock,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    if expected.repository != SDK_REPOSITORY || expected.target != "aarch64-apple-darwin" {
        return Err(internal("sdk_release_selection_invalid"));
    }
    let velnor_actions_contract::runner_image_manifest::mac_attestation::MacSdkAuthority::Stable {
        release_manifest_sha256,
    } = &expected.authority
    else {
        return Err(internal("sdk_release_selection_invalid"));
    };
    let candidate = load_sdk_release_candidate(
        &expected.version,
        &expected.source_commit,
        release_manifest_sha256,
        deadline,
    )?;
    velnor_actions_mise::catalog::lock::verify_lock_against_manifest(
        generator_lock,
        &candidate.manifest,
    )
    .map_err(|_| internal("sdk_release_generator_lock_mismatch"))?;
    let target = candidate
        .manifest
        .record_for_target(&expected.target)
        .ok_or_else(|| internal("sdk_release_host_target_missing"))?;
    if target.sha256 != expected.asset_sha256 {
        return Err(internal("sdk_release_binary_manifest_mismatch"));
    }
    verify_sdk_release_attestation(
        &candidate,
        &expected.source_commit,
        release_manifest_sha256,
        deadline,
    )
}

fn validate_selection(
    transition: &velnor_actions_contract::homebrew_adoption::HomebrewAdoptionTransition,
    version: &str,
    source_sha: &str,
    manifest_sha256: &str,
) -> Result<(), OrchestratorError> {
    if !velnor_actions_contract::config::is_exact_semver(version)
        || !velnor_actions_contract::config::is_exact_semver(&transition.minimum_sdk_version)
        || compare_stable_versions(version, &transition.minimum_sdk_version)?
            == std::cmp::Ordering::Less
        || !is_sha(source_sha)
        || !is_sha256(manifest_sha256)
        || transition.required_sdk_capability != SDK_CAPABILITY
    {
        return Err(internal("sdk_release_selection_invalid"));
    }
    Ok(())
}

#[path = "homebrew_adoption_sdk_binding_assets.rs"]
mod assets;
pub(crate) use assets::sha256_current_executable;
#[path = "homebrew_adoption_sdk_binding_digest.rs"]
mod digest;
use assets::{
    load_sdk_release_candidate, verify_published_adoption_callee, verify_sdk_release_attestation,
};
use digest::{is_sha, is_sha256};

fn compare_stable_versions(
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
