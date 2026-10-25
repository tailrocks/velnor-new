//! Private binding from an immutable published SDK release to renderer inputs.

use velnor_actions_mise::CheckDeadline;

use crate::{
    OrchestratorError, homebrew::adoption::ProtectedHomebrewAdoptionTransition, internal::internal,
};

const SDK_REPOSITORY: &str = "tailrocks/velnor-new";
const SDK_REPOSITORY_ID: u64 = 1_390_620_900;
const SDK_OWNER_ID: u64 = 78_806_509;
const SDK_CAPABILITY: &str = "homebrew-adoption-seed-v1";
const MAX_MANIFEST_BYTES: usize = 8 * 1024 * 1024;
const MAX_CALLEE_BYTES: usize = 2 * 1024 * 1024;
const MAX_BINARY_BYTES: u64 = 128 * 1024 * 1024;
const MAX_SIDECAR_BYTES: u64 = 1024;
const MAX_PROVENANCE_BYTES: u64 = 64 * 1024;
const MAX_ATTESTATION_BYTES: u64 = 1024 * 1024;

#[cfg(test)]
#[path = "homebrew_adoption_sdk_binding_tests.rs"]
mod tests;

/// Authenticated values safe to pass to the pure seed renderer.
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

/// Private custody for a separately published, immutable SDK release.
///
/// This value has no deserializer or public constructor. It can be created only
/// after release bytes, manifest attestation, producer run, and protected-main
/// lineage pass independent public verification.
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

/// Select and authenticate an external SDK release using data from the
/// reviewed seed input. Every supplied value is treated as untrusted until it
/// matches the immutable GitHub release and its verified manifest attestation.
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
#[path = "homebrew_adoption_sdk_binding_digest.rs"]
mod digest;
use assets::{
    compare_stable_versions, load_sdk_release_candidate, verify_published_adoption_callee,
    verify_sdk_release_attestation,
};
use digest::{is_sha, is_sha256};
