//! Authenticated, non-overwriting GitHub release publication.

use std::{collections::BTreeSet, env};
use velnor_actions_contract::{
    package_release::{PackageReleaseChannel, PackageReleaseReceipt},
    publication_retry::{PublicationIdentity, PublicationState},
};

use crate::{
    OrchestratorError,
    internal::internal,
    package_publication::{
        PackagePublicationBackend, PackagePublicationOutcome, publish_package_release,
    },
    package_release_signer::{
        PackageReleaseSigningIdentity, VerifiedAttestedSignedPackage,
        verify_attested_signed_package, verify_attested_signed_package_from_env,
    },
};

#[path = "package_release_publish_identity.rs"]
mod identity;
use identity::{
    combine_origin_identities, draft_receipt_digest, is_git_sha, local_identity, read_regular,
    recovery_body, tag_receipt_digest,
};

#[path = "package_release_publish_github.rs"]
mod github;
use github::{
    BranchResponse, GitHubApi, GitRefResponse, ReleaseResponse, TagObservation, encode_segment,
    sha256, unique_release,
};

#[path = "package_release_run_identity.rs"]
mod run_identity;
use run_identity::RunSource;

#[path = "artifact_zip.rs"]
pub(crate) mod artifact_zip;
#[path = "preview_artifact_transport.rs"]
mod preview_artifacts;
pub(crate) use preview_artifacts::{
    AdmittedPreviewArtifact, acquire_preview_aggregate, acquire_preview_artifacts,
};

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

#[path = "package_release_publish_recovery.rs"]
mod recovery;
pub(crate) use recovery::verify_original_for_promotion;

/// Reverify the downloaded signed package, then execute the GitHub publication transaction.
/// # Errors
/// Rejects absent credentials, invalid original evidence, stale candidates, and conflicting remote state.
pub fn publish_package_release_from_env() -> Result<PackagePublicationOutcome, OrchestratorError> {
    let signed = verify_attested_signed_package_from_env()?;
    let current_ci = current_ci_from_env()?;
    let mut backend =
        GitHubPackagePublisher::new(signed, current_ci, OriginalVerification::RunnerEnvironment)?;
    publish_package_release(&mut backend)
}

/// Resume release publication from an authenticated original workflow artifact.
/// # Errors
/// Rejects an invalid retained run/artifact, stale candidate, or conflicting remote state.
pub fn recover_package_release_from_env() -> Result<PackagePublicationOutcome, OrchestratorError> {
    artifact_zip::within_operation(recovery::recover)
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
    original_verification: OriginalVerification,
}

impl GitHubPackagePublisher {
    fn new(
        signed: VerifiedAttestedSignedPackage,
        current_ci: (u64, u32),
        original_verification: OriginalVerification,
    ) -> Result<Self, OrchestratorError> {
        let receipt = signed.receipt().clone();
        let identity = local_identity(&signed)?;
        let recovery_body = recovery_body(&signed, &identity.receipt_digest)?;
        let api = GitHubApi::new(receipt.repository.slug.clone());
        Ok(Self {
            signed,
            api,
            identity,
            tag: receipt.tag.clone(),
            recovery_body,
            receipt,
            current_ci,
            original_verification,
        })
    }

    fn release_list(&self) -> Result<Vec<ReleaseResponse>, OrchestratorError> {
        self.api.release_list(&self.tag)
    }

    fn verify_remote_repository(&self) -> Result<(), OrchestratorError> {
        self.api.verify_repository(self.receipt.repository.id)
    }

    fn remote_state(
        &self,
        release: &ReleaseResponse,
        tag: TagObservation,
    ) -> Result<PublicationState, OrchestratorError> {
        let assets = self.api.remote_assets(release)?;
        if release.draft == Some(true) {
            let draft_identity = draft_receipt_digest(
                release.body.as_deref(),
                release.author.as_ref(),
                &self.recovery_body,
                &self.identity.receipt_digest,
            );
            let tag_identity = tag_receipt_digest(
                tag.message.as_deref(),
                &self.recovery_body,
                &self.identity.receipt_digest,
            );
            let original_receipt_digest = combine_origin_identities(draft_identity, tag_identity);
            return Ok(PublicationState::Draft {
                source_sha: tag.source_sha,
                original_receipt_digest,
                assets,
            });
        }
        if release.draft != Some(false) {
            return Err(internal("package_publish_release_state_invalid"));
        }
        let expected_prerelease = self.receipt.channel == PackageReleaseChannel::Preview;
        let trusted_tag = tag_receipt_digest(
            tag.message.as_deref(),
            &self.recovery_body,
            &self.identity.receipt_digest,
        )
        .as_deref()
            == Some(self.identity.receipt_digest.as_str());
        let identity = PublicationIdentity {
            source_sha: tag.source_sha,
            receipt_digest: if trusted_tag {
                assets.get("release.json").cloned().unwrap_or_default()
            } else {
                "untrusted-tag-envelope".to_owned()
            },
            assets,
        };
        Ok(PublicationState::Published {
            identity,
            immutable: release.immutable == Some(true)
                && release.prerelease == Some(expected_prerelease),
        })
    }
}

impl PackagePublicationBackend for GitHubPackagePublisher {
    fn authenticate_original(&mut self) -> Result<PublicationIdentity, OrchestratorError> {
        self.signed = match &self.original_verification {
            OriginalVerification::RunnerEnvironment => verify_attested_signed_package_from_env()?,
            OriginalVerification::RetainedRun {
                directory,
                plan,
                identity,
            } => verify_attested_signed_package(directory, plan, identity)?,
        };
        let identity = local_identity(&self.signed)?;
        if identity != self.identity || self.signed.receipt() != &self.receipt {
            return Err(internal("package_publish_original_changed"));
        }
        self.api.verify_release_run(
            &self.receipt,
            matches!(
                &self.original_verification,
                OriginalVerification::RetainedRun { .. }
            ),
        )?;
        Ok(identity)
    }

    fn observe(&mut self) -> Result<PublicationState, OrchestratorError> {
        self.verify_remote_repository()?;
        let tag = self.api.tag_identity(&self.tag)?;
        let releases = self.release_list()?;
        if releases.len() > 1 {
            return Err(internal("package_publish_duplicate_release_tag"));
        }
        let Some(release) = releases.first() else {
            return Ok(match tag {
                Some(tag) => PublicationState::TagOnly {
                    source_sha: tag.source_sha,
                    original_receipt_digest: tag_receipt_digest(
                        tag.message.as_deref(),
                        &self.recovery_body,
                        &self.identity.receipt_digest,
                    ),
                },
                None => PublicationState::Absent,
            });
        };
        let Some(tag) = tag else {
            return Err(internal("package_publish_release_tag_missing"));
        };
        self.remote_state(release, tag)
    }

    fn available_original_assets(&mut self) -> Result<BTreeSet<String>, OrchestratorError> {
        Ok(local_identity(&self.signed)?.assets.into_keys().collect())
    }

    fn candidate_current(&mut self) -> Result<bool, OrchestratorError> {
        self.verify_remote_repository()?;
        self.api.verify_release_channel_environment(
            self.receipt.channel,
            &self.receipt.source.default_branch,
        )?;
        let branch = encode_segment(&self.receipt.source.default_branch);
        let branch_response: BranchResponse = self
            .api
            .get_json(&self.api.route(&format!("branches/{branch}")))?;
        if branch_response.protected != Some(true) {
            return Ok(false);
        }
        let branch_ref = format!(
            "heads/{}",
            encode_segment(&self.receipt.source.default_branch)
        );
        let source = RunSource {
            repository_id: self.receipt.repository.id,
            sha: &self.receipt.source.commit_sha,
            branch: &self.receipt.source.default_branch,
        };
        self.api.verify_ci_identity(
            self.receipt.source.ci_run_id,
            self.receipt.source.ci_run_attempt,
            source,
            false,
        )?;
        self.api
            .verify_ci_identity(self.current_ci.0, self.current_ci.1, source, true)?;
        let current: GitRefResponse = self
            .api
            .get_json(&self.api.route(&format!("git/ref/{branch_ref}")))?;
        Ok(current.object.object_type == "commit"
            && current.object.sha == self.receipt.source.commit_sha)
    }

    fn create_tag(&mut self) -> Result<(), OrchestratorError> {
        let object = self.api.create_tag_object(
            &self.tag,
            &self.recovery_body,
            &self.receipt.source.commit_sha,
        )?;
        if object.tag != self.tag
            || object.message != self.recovery_body
            || object.object.object_type != "commit"
            || object.object.sha != self.receipt.source.commit_sha
            || !is_git_sha(&object.sha)
        {
            return Err(internal("package_publish_tag_object_postcondition"));
        }
        if !self.candidate_current()? {
            return Err(internal("package_publish_tag_stale_before_ref"));
        }
        self.api.post_fields(
            &self.api.route("git/refs"),
            &[
                ("ref", format!("refs/tags/{}", self.tag)),
                ("sha", object.sha),
            ],
        )?;
        Ok(())
    }

    fn create_draft(&mut self) -> Result<(), OrchestratorError> {
        let prerelease = self.receipt.channel == PackageReleaseChannel::Preview;
        self.api.post_fields(
            &self.api.route("releases"),
            &[
                ("tag_name", self.tag.clone()),
                ("target_commitish", self.receipt.source.commit_sha.clone()),
                ("name", self.receipt.version.value.clone()),
                ("body", self.recovery_body.clone()),
                ("draft", "true".to_owned()),
                ("prerelease", prerelease.to_string()),
                ("make_latest", "false".to_owned()),
            ],
        )?;
        Ok(())
    }

    fn upload_original_asset(&mut self, name: &str) -> Result<(), OrchestratorError> {
        let expected = self
            .identity
            .assets
            .get(name)
            .ok_or_else(|| internal("package_publish_asset_unexpected"))?;
        let path = self.signed.directory().join(name);
        let bytes = read_regular(&path)?;
        if &sha256(&bytes) != expected {
            return Err(internal("package_publish_original_asset_changed"));
        }
        let releases = self.release_list()?;
        let release = unique_release(&releases, &self.tag)?;
        let route = format!(
            "{}?name={}",
            self.api.route(&format!("releases/{}/assets", release.id)),
            encode_segment(name)
        );
        self.api.upload_file(route, &path)
    }

    fn publish_draft(&mut self) -> Result<(), OrchestratorError> {
        let releases = self.release_list()?;
        let release = unique_release(&releases, &self.tag)?;
        let route = self.api.route(&format!("releases/{}", release.id));
        let prerelease = self.receipt.channel == PackageReleaseChannel::Preview;
        let make_latest = make_latest(self.receipt.channel);
        self.api.patch_fields(
            &route,
            &[
                ("draft", "false".to_owned()),
                ("prerelease", prerelease.to_string()),
                ("make_latest", make_latest.to_string()),
            ],
        )?;
        let updated: ReleaseResponse = self.api.get_json(&route)?;
        if updated.draft != Some(false) || updated.prerelease != Some(prerelease) {
            return Err(internal("package_publish_release_postcondition"));
        }
        Ok(())
    }
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
