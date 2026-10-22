//! Immutable publication transport; permanent tags belong to the native metadata job.

use super::{
    GitHubPackagePublisher, OrchestratorError, OriginalVerification,
    github::{ReleaseResponse, encode_segment, sha256},
    identity::{local_identity, read_regular},
    internal, make_latest, prerelease,
    run_identity::RunSource,
};
use crate::package_publication::PackagePublicationBackend;
use std::collections::BTreeSet;
#[path = "package_release_publish_stable.rs"]
pub(super) mod stable;
use velnor_actions_contract::{
    CI_WORKFLOW_PATH, WORKFLOW_DISPLAY_NAME,
    package_release::PackageReleaseChannel,
    publication_retry::{PublicationIdentity, PublicationState},
};

impl PackagePublicationBackend for GitHubPackagePublisher {
    fn authenticate_original(&mut self) -> Result<PublicationIdentity, OrchestratorError> {
        self.original_closure.verify()?;
        match &self.original_verification {
            OriginalVerification::RunnerEnvironment => {
                crate::package_release_signer::recheck_publisher_context(&self.signed)?;
            }
            OriginalVerification::RetainedRun {
                directory,
                plan,
                identity,
            } => {
                if directory.as_path() != self.signed.directory()
                    || plan.as_ref() != self.signed.plan()
                {
                    return Err(internal("package_publish_original_changed"));
                }
                crate::package_release_signer::require_retained_identity(&self.signed, identity)?;
            }
        }
        let identity = local_identity(&self.signed)?;
        if identity != self.identity || self.signed.receipt() != &self.receipt {
            return Err(internal("package_publish_original_changed"));
        }
        if self.receipt.channel == PackageReleaseChannel::Stable {
            self.verify_stable_origin()?;
        }
        Ok(identity)
    }

    fn observe(&mut self) -> Result<PublicationState, OrchestratorError> {
        self.stable_observation = None;
        self.verify_remote_repository()?;
        if self.receipt.channel == PackageReleaseChannel::Stable {
            self.verify_stable_origin()?;
        }
        let tag = self.api.tag_identity(&self.tag)?;
        let Some(mut release) = self.release_by_tag()? else {
            let Some(tag) = tag else {
                if self.receipt.channel == PackageReleaseChannel::Preview {
                    let state = PublicationState::Absent;
                    self.stable_observation = Some(stable::capture(&state, None));
                    return Ok(state);
                }
                return Err(internal("stable_authorized_tag_missing"));
            };
            if tag.source_sha != self.identity.source_sha {
                return Err(internal("stable_authorized_tag_source_changed"));
            }
            let state = match self.receipt.channel {
                PackageReleaseChannel::Stable => PublicationState::AuthorizedStableTag {
                    identity: self.identity.clone(),
                },
                PackageReleaseChannel::Preview
                    if tag.message.as_deref() == Some(self.recovery_body.as_str()) =>
                {
                    PublicationState::TagOnly {
                        source_sha: tag.source_sha,
                        original_receipt_digest: Some(self.identity.receipt_digest.clone()),
                    }
                }
                PackageReleaseChannel::Preview => PublicationState::TagOnly {
                    source_sha: tag.source_sha,
                    original_receipt_digest: None,
                },
            };
            self.stable_observation = Some(stable::capture(&state, None));
            return Ok(state);
        };
        let tag = tag.ok_or_else(|| internal("package_publish_release_tag_missing"))?;
        release.assets = self.api.complete_release_assets(release.id)?;
        let state = self.remote_state(&release, tag)?;
        self.stable_observation = Some(stable::capture(&state, Some(&release)));
        Ok(state)
    }

    fn available_original_assets(&mut self) -> Result<BTreeSet<String>, OrchestratorError> {
        Ok(local_identity(&self.signed)?.assets.into_keys().collect())
    }

    fn candidate_current(&mut self) -> Result<bool, OrchestratorError> {
        self.api.verify_current_release_run(
            &self.receipt,
            matches!(
                &self.original_verification,
                OriginalVerification::RetainedRun { .. }
            ),
        )?;
        if matches!(
            &self.original_verification,
            OriginalVerification::RetainedRun { .. }
        ) {
            super::recovery::promotion_context::Invocation::from_env()?.authenticate_recovery(
                self.signed.plan(),
                &self.api,
                self.receipt.repository.id,
            )?;
        }
        self.verify_remote_repository()?;
        if self.receipt.channel == PackageReleaseChannel::Stable {
            self.candidate_current_stable()?;
            return Ok(true);
        }
        self.candidate_current_preview()
    }

    fn create_tag(&mut self) -> Result<(), OrchestratorError> {
        if self.receipt.channel != PackageReleaseChannel::Preview {
            return Err(internal("stable_tag_owned_by_native_metadata_job"));
        }
        let observed = stable::take(self, &stable::Action::Tag)?;
        stable::authorize(self, observed, true)?;
        self.original_closure.verify()?;
        let object = self.api.create_tag_object(
            &self.tag,
            &self.recovery_body,
            &self.receipt.source.commit_sha,
        )?;
        if object.tag != self.tag
            || object.message != self.recovery_body
            || object.object.object_type != "commit"
            || object.object.sha != self.receipt.source.commit_sha
            || !super::github::is_lowercase_git_sha(&object.sha)
        {
            return Err(internal("preview_tag_object_invalid"));
        }
        self.original_closure.verify()?;
        if self.api.tag_identity(&self.tag)?.is_some() || self.release_by_tag()?.is_some() {
            return Err(internal("package_publication_refused:ConflictingTag"));
        }
        self.original_closure.verify()?;
        if !PackagePublicationBackend::candidate_current(self)? {
            return Err(internal("package_publication_refused:StaleCandidate"));
        }
        self.api.post_fields(
            &self.api.route("git/refs"),
            &[
                ("ref", format!("refs/tags/{}", self.tag)),
                ("sha", object.sha),
            ],
        )?;
        let observed = self
            .api
            .tag_identity(&self.tag)?
            .ok_or_else(|| internal("preview_tag_created_missing"))?;
        if observed.source_sha != self.receipt.source.commit_sha
            || observed.message.as_deref() != Some(self.recovery_body.as_str())
        {
            return Err(internal("preview_tag_created_conflict"));
        }
        Ok(())
    }

    fn create_draft(&mut self) -> Result<(), OrchestratorError> {
        let observed = stable::take(self, &stable::Action::Draft)?;
        stable::authorize(self, observed, false)?;
        let prerelease = prerelease(self.receipt.channel);
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
        let observed = stable::take(self, &stable::Action::Upload(name))?;
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
        let release_id = stable::authorize(self, observed, false)?
            .ok_or_else(|| internal("stable_write_release_missing"))?;
        let route = format!(
            "{}?name={}",
            self.api.route(&format!("releases/{release_id}/assets")),
            encode_segment(name)
        );
        self.api.upload_file(&route, &path)
    }

    fn publish_draft(&mut self) -> Result<(), OrchestratorError> {
        let observed = stable::take(self, &stable::Action::Publish)?;
        let release_id = stable::authorize(self, observed, false)?
            .ok_or_else(|| internal("stable_write_release_missing"))?;
        let route = self.api.route(&format!("releases/{release_id}"));
        let prerelease = prerelease(self.receipt.channel);
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

impl GitHubPackagePublisher {
    fn candidate_current_stable(&mut self) -> Result<(), OrchestratorError> {
        self.api.verify_release_channel_environment(
            self.receipt.channel,
            &self.receipt.source.default_branch,
        )?;
        self.verify_stable_origin()?;
        stable::verify_current_ci(self)?;
        self.stable_origin
            .as_ref()
            .ok_or_else(|| internal("stable_origin_missing"))?
            .verify_tag(&self.api)?;
        self.original_closure.verify()
    }

    fn candidate_current_preview(&mut self) -> Result<bool, OrchestratorError> {
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
        // Preserve the signed original CI identity above while independently requiring
        // the latest exact-source attempt to be successful. A later successful rerun
        // is valid; a pending or failed latest attempt vetoes publication/recovery.
        let source_workflow = self
            .api
            .admitted_workflow(CI_WORKFLOW_PATH, WORKFLOW_DISPLAY_NAME)?;
        let latest_source = self.api.latest_source_ci(&source_workflow, source)?;
        let latest_id = latest_source
            .id
            .filter(|id| *id > 0)
            .ok_or_else(|| internal("package_ci_latest_attempt_mismatch"))?;
        let latest_attempt = latest_source
            .run_attempt
            .filter(|attempt| *attempt > 0)
            .ok_or_else(|| internal("package_ci_latest_attempt_mismatch"))?;
        self.api
            .verify_ci_identity(latest_id, latest_attempt, source, true)?;
        let retained = matches!(
            &self.original_verification,
            OriginalVerification::RetainedRun { .. }
        );
        let coordinator_sha = if retained {
            super::recovery::promotion_context::Invocation::from_env()?.source_sha
        } else {
            self.receipt.workflow.authority_sha.clone()
        };
        verify_current_preview_ci(
            &self.api,
            self.receipt.repository.id,
            &self.receipt.source,
            self.current_ci,
        )?;
        self.api.verify_preview_main_ancestry(
            self.receipt.repository.id,
            &self.receipt.source.default_branch,
            &self.receipt.source.commit_sha,
            &self.receipt.workflow.authority_sha,
        )?;
        if coordinator_sha != self.receipt.workflow.authority_sha {
            self.api.verify_preview_main_ancestry(
                self.receipt.repository.id,
                &self.receipt.source.default_branch,
                &self.receipt.source.commit_sha,
                &coordinator_sha,
            )?;
        }
        self.original_closure.verify()?;
        Ok(true)
    }
}

fn verify_current_preview_ci(
    api: &super::github::GitHubApi,
    repository_id: u64,
    source: &velnor_actions_contract::package_release::PackageReleaseSource,
    current_ci: (u64, u32),
) -> Result<(), OrchestratorError> {
    api.verify_ci_identity(
        current_ci.0,
        current_ci.1,
        RunSource {
            repository_id,
            sha: &source.commit_sha,
            branch: &source.default_branch,
        },
        true,
    )
}

#[cfg(test)]
#[path = "package_release_publish_backend_tests.rs"]
mod tests;
