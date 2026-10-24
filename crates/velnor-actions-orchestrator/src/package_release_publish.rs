//! Authenticated, non-overwriting GitHub release publication.

use crate::{
    OrchestratorError,
    internal::internal,
    package_publication::{PackagePublicationOutcome, publish_package_release},
    package_release_signer::{
        PackageReleaseSigningIdentity, VerifiedAttestedSignedPackage,
        verify_attested_signed_package_from_env_with_deadline,
    },
};
use std::env;
use velnor_actions_contract::{
    package_release::{PackageReleaseChannel, PackageReleaseReceipt},
    publication_retry::{PublicationIdentity, PublicationState},
};

#[path = "release_policy_admission.rs"]
pub(crate) mod release_policy_admission;

#[path = "stable_tag_origin.rs"]
pub(crate) mod stable_tag_origin;
pub use stable_tag_origin::{
    ADMIT_STABLE_TAG_ORIGIN_OP, SELECT_STABLE_RELEASE_SOURCE_OP, WRITE_STABLE_TAG_INTENT_OP,
    WRITE_STABLE_TAG_ORIGIN_OP, admit_stable_tag_origin_from_env,
    select_stable_release_source_from_env, write_stable_tag_intent_from_env,
    write_stable_tag_origin_from_env,
};

#[path = "package_release_published.rs"]
mod published;
pub(crate) use published::verify_published_release;

#[path = "package_release_publish_source.rs"]
mod source;
pub(crate) use source::{
    ProtectedSourceTreeIdentity, read_protected_adoption_seed_base_tree_identity,
    read_protected_source_file, read_protected_source_tree_identity,
    try_read_protected_source_file, verify_preview_main_tip, verify_public_protected_main_lineage,
    verify_public_pull_request_lineage,
};

#[path = "package_release_publish_backend.rs"]
mod backend;

#[path = "package_release_publish_identity.rs"]
mod identity;
use identity::{draft_receipt_digest, local_identity, recovery_body};

#[path = "package_release_publish_github.rs"]
mod github;
use github::{
    GitHubApi, ReleaseAssetResponse, ReleaseResponse, TagObservation, WorkflowMetadata,
    WorkflowRunResponse, sha256,
};

#[path = "package_release_publish_sdk.rs"]
mod sdk;
pub(crate) use sdk::{
    PublicSdkCallerSelection, PublicSdkProducerSelection, verify_public_sdk_adoption_caller,
    verify_public_sdk_release_producer,
};

#[path = "package_release_run_identity.rs"]
mod run_identity;

#[path = "artifact_zip.rs"]
pub(crate) mod artifact_zip;
#[path = "package_release_publish_budget.rs"]
pub(crate) mod budget;
#[path = "preview_artifact_transport.rs"]
mod preview_artifacts;
#[cfg(test)]
pub(crate) use preview_artifacts::tests::discover_request_for_sealing_test;
pub(crate) use preview_artifacts::{
    AdmittedReleaseArtifact, acquire_release_aggregate, acquire_release_artifacts,
};

#[path = "package_release_publish_public_read.rs"]
mod public_read;
pub(crate) use public_read::{
    PublicPullRequestSnapshot, PublicReleaseAsset, PublicReleaseSnapshot, read_public_branch_tip,
    read_public_pull_request, read_public_release_snapshot, read_public_repository_identity,
};

#[path = "package_release_publish_public_recovery.rs"]
mod public_recovery;

#[cfg(test)]
use public_read::validate_public_asset_size_inventory;
#[cfg(test)]
use public_recovery::{verify_public_recovery_jobs, verify_public_recovery_original_jobs};
#[cfg(test)]
use public_runs::{public_release_workflow_matches, verify_public_release_producer_jobs};

#[path = "package_release_publish_public_runs.rs"]
mod public_runs;
pub(crate) use public_runs::{
    verify_public_historical_release_recovery, verify_public_release_runs,
};

/// Reuse the existing immutable publication predicates without authenticating a
/// promotion invocation or exposing the write-capable GitHub API adapter.
pub(crate) fn verify_publication_snapshot(
    signed: &VerifiedAttestedSignedPackage,
    snapshot: &PublicReleaseSnapshot,
) -> Result<(), OrchestratorError> {
    published::verify_publication_snapshot(signed, snapshot)
}

fn asset_tuples(assets: &[ReleaseAssetResponse]) -> Vec<(u64, String, u64, Option<String>)> {
    let mut tuples = assets
        .iter()
        .map(|asset| {
            (
                asset.id,
                asset.name.clone(),
                asset.size,
                asset.digest.clone(),
            )
        })
        .collect::<Vec<_>>();
    tuples.sort();
    tuples
}

fn asset_tuples_from_downloads(
    assets: &[PublicReleaseAsset],
) -> Vec<(u64, String, u64, Option<String>)> {
    let mut tuples = assets
        .iter()
        .map(|asset| {
            (
                asset.id,
                asset.name.clone(),
                asset.size,
                asset.api_digest.clone(),
            )
        })
        .collect::<Vec<_>>();
    tuples.sort();
    tuples
}

/// Private operation that publishes one authenticated native release.
pub const PUBLISH_PACKAGE_RELEASE_OP: &str = "publish-package-release-v1";
/// Private operation that resumes publication from the exact retained signed preview artifact.
pub const RECOVER_PACKAGE_RELEASE_OP: &str = "recover-package-release-v1";

/// Recheck the source repository's main-only tap-update environment.
/// # Errors
/// Rejects an absent or invalid source-environment policy.
pub(crate) fn verify_tap_update_environment(
    repository: &str,
    branch: &str,
) -> Result<(), OrchestratorError> {
    github::verify_tap_update_environment(repository, branch)
}

#[path = "package_release_continuation.rs"]
mod continuation;
pub(crate) use continuation::verify_current_source;
pub use continuation::{
    ReleaseTapContinuationSelection, SELECT_RELEASE_TAP_CONTINUATION_OP,
    select_release_tap_continuation_from_env,
};

#[path = "package_release_publish_recovery.rs"]
mod recovery;
pub(crate) use recovery::verify_original_for_promotion;

/// Reverify the downloaded signed package, then execute the GitHub publication transaction.
/// # Errors
/// Rejects absent credentials, invalid original evidence, stale candidates, and conflicting remote state.
pub fn publish_package_release_from_env() -> Result<PackagePublicationOutcome, OrchestratorError> {
    budget::with_env(|deadline| {
        let mut backend = artifact_zip::within_operation(|| {
            let signed = verify_attested_signed_package_from_env_with_deadline(deadline)?;
            let current_ci = current_ci_from_env()?;
            GitHubPackagePublisher::new(signed, current_ci, OriginalVerification::RunnerEnvironment)
        })?;
        publish_backend(&mut backend)
    })
}

fn publish_backend(
    backend: &mut GitHubPackagePublisher,
) -> Result<PackagePublicationOutcome, OrchestratorError> {
    let outcome = publish_package_release(backend)?;
    published::verify_published_release(&backend.signed)?;
    Ok(outcome)
}

/// Resume release publication from an authenticated original workflow artifact.
/// # Errors
/// Rejects an invalid retained run/artifact, stale candidate, or conflicting remote state.
pub fn recover_package_release_from_env() -> Result<PackagePublicationOutcome, OrchestratorError> {
    budget::with_env(recovery::recover)
}

#[derive(Clone)]
enum OriginalVerification {
    RunnerEnvironment,
    RetainedRun {
        directory: Box<std::path::PathBuf>,
        plan: Box<velnor_actions_contract::PackageReleasePlan>,
        identity: Box<PackageReleaseSigningIdentity>,
    },
}

struct GitHubPackagePublisher {
    signed: VerifiedAttestedSignedPackage,
    api: GitHubApi,
    identity: PublicationIdentity,
    receipt: PackageReleaseReceipt,
    tag: String,
    recovery_body: String,
    current_ci: (u64, u32),
    stable_origin: Option<stable_tag_origin::StableOriginProof>,
    stable_observation: Option<backend::stable::Observation>,
    original_verification: OriginalVerification,
    original_closure: std::sync::Arc<crate::filesystem_owner::held_directory::HeldDirectory>,
}

impl GitHubPackagePublisher {
    fn new(
        signed: VerifiedAttestedSignedPackage,
        current_ci: (u64, u32),
        original_verification: OriginalVerification,
    ) -> Result<Self, OrchestratorError> {
        let original_closure = signed.closure();
        original_closure.verify()?;
        let receipt = signed.receipt().clone();
        let identity = local_identity(&signed)?;
        let recovery_body = recovery_body(&signed, &identity.receipt_digest)?;
        original_closure.verify()?;
        let api = GitHubApi::new(receipt.repository.slug.clone());
        Ok(Self {
            signed,
            api,
            identity,
            tag: receipt.tag.clone(),
            recovery_body,
            receipt,
            current_ci,
            stable_origin: None,
            stable_observation: None,
            original_verification,
            original_closure,
        })
    }

    fn verify_stable_origin(&mut self) -> Result<(), OrchestratorError> {
        if self.stable_origin.is_none() {
            let proof = stable_tag_origin::admit_signed_origin(&self.api, &self.signed)?;
            self.stable_origin = Some(proof);
        }
        self.stable_origin
            .as_ref()
            .ok_or_else(|| internal("stable_origin_missing"))?
            .verify_local(&self.signed)
    }

    fn release_by_tag(&self) -> Result<Option<ReleaseResponse>, OrchestratorError> {
        self.api.release_by_tag(&self.tag)
    }

    fn verify_remote_repository(&self) -> Result<(), OrchestratorError> {
        self.api.verify_repository(self.receipt.repository.id)
    }

    fn remote_state(
        &mut self,
        release: &ReleaseResponse,
        tag: TagObservation,
    ) -> Result<PublicationState, OrchestratorError> {
        admit_release_lifecycle(release, &self.tag, self.receipt.channel)?;
        if self.receipt.channel == PackageReleaseChannel::Stable {
            self.verify_stable_origin()?;
        } else if tag.source_sha != self.identity.source_sha
            || tag.message.as_deref() != Some(self.recovery_body.as_str())
        {
            return Err(internal("preview_release_tag_origin_changed"));
        }
        let assets = self.api.remote_assets(release)?;
        if release.draft == Some(true) {
            return Ok(PublicationState::Draft {
                source_sha: tag.source_sha,
                original_receipt_digest: draft_receipt_digest(
                    release.body.as_deref(),
                    release.author.as_ref(),
                    &self.recovery_body,
                    &self.identity.receipt_digest,
                ),
                assets,
            });
        }
        if release.draft != Some(false) {
            return Err(internal("package_publish_release_state_invalid"));
        }
        if release.body.as_deref() != Some(self.recovery_body.as_str()) {
            return Err(internal("package_published_envelope_changed"));
        }
        Ok(PublicationState::Published {
            identity: PublicationIdentity {
                source_sha: tag.source_sha,
                receipt_digest: assets.get("release.json").cloned().unwrap_or_default(),
                assets,
            },
            immutable: release.immutable == Some(true)
                && release.prerelease
                    == Some(self.receipt.channel == PackageReleaseChannel::Preview),
        })
    }
}

fn admit_release_lifecycle(
    release: &ReleaseResponse,
    expected_tag: &str,
    channel: PackageReleaseChannel,
) -> Result<(), OrchestratorError> {
    if release.id == 0
        || release.tag_name != expected_tag
        || release.prerelease != Some(channel == PackageReleaseChannel::Preview)
        || release.draft.is_none()
        || (release.draft == Some(true) && release.immutable != Some(false))
    {
        return Err(internal("package_release_lifecycle_invalid"));
    }
    // Published immutability is independently required by the publication planner.
    Ok(())
}

fn prerelease(channel: PackageReleaseChannel) -> bool {
    channel == PackageReleaseChannel::Preview
}

fn make_latest(_channel: PackageReleaseChannel) -> bool {
    false
}

fn current_ci_from_env() -> Result<(u64, u32), OrchestratorError> {
    let run_id = env::var("VELNOR_RELEASE_CI_RUN_ID")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| internal("package_publish_current_ci_run_invalid"))?;
    let attempt = env::var("VELNOR_RELEASE_CI_RUN_ATTEMPT")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| internal("package_publish_current_ci_attempt_invalid"))?;
    Ok((run_id, attempt))
}

#[cfg(test)]
#[path = "package_release_publish_tests.rs"]
mod tests;
