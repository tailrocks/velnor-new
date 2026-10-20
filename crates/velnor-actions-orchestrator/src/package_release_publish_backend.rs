//! Immutable publication transport; permanent tags belong to the native metadata job.

use super::{
    GitHubPackagePublisher, OrchestratorError, OriginalVerification,
    github::{
        BranchResponse, GitRefResponse, ReleaseResponse, encode_segment, sha256, unique_release,
    },
    identity::{local_identity, read_regular},
    internal, make_latest, require_stable,
    run_identity::RunSource,
    stable_tag_origin, verify_attested_signed_package, verify_attested_signed_package_from_env,
};
use crate::package_publication::PackagePublicationBackend;
use std::collections::BTreeSet;
use velnor_actions_contract::{
    package_release::PackageReleaseChannel,
    publication_retry::{PublicationIdentity, PublicationState},
};

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
        if self.receipt.channel == PackageReleaseChannel::Stable {
            stable_tag_origin::verify_signed_origin(&self.signed)?;
        }
        Ok(identity)
    }

    fn observe(&mut self) -> Result<PublicationState, OrchestratorError> {
        require_stable(self.receipt.channel)?;
        self.verify_remote_repository()?;
        let tag = self.api.tag_identity(&self.tag)?;
        if self.receipt.channel == PackageReleaseChannel::Stable && tag.is_none() {
            return Err(internal("stable_authorized_tag_missing"));
        }
        let releases = self.release_list()?;
        if releases.len() > 1 {
            return Err(internal("package_publish_duplicate_release_tag"));
        }
        let Some(release) = releases.first() else {
            let tag = tag.ok_or_else(|| internal("stable_authorized_tag_missing"))?;
            stable_tag_origin::verify_signed_origin(&self.signed)?;
            if tag.source_sha != self.identity.source_sha {
                return Err(internal("stable_authorized_tag_source_changed"));
            }
            return Ok(PublicationState::AuthorizedStableTag {
                identity: self.identity.clone(),
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
        if self.receipt.channel == PackageReleaseChannel::Stable {
            stable_tag_origin::verify_signed_origin(&self.signed)?;
            return Ok(true);
        }
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
        Err(internal("stable_tag_owned_by_native_metadata_job"))
    }

    fn create_draft(&mut self) -> Result<(), OrchestratorError> {
        require_stable(self.receipt.channel)?;
        let prerelease = false;
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
        require_stable(self.receipt.channel)?;
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
        self.api.upload_file(&route, &path)
    }

    fn publish_draft(&mut self) -> Result<(), OrchestratorError> {
        let releases = self.release_list()?;
        let release = unique_release(&releases, &self.tag)?;
        let route = self.api.route(&format!("releases/{}", release.id));
        require_stable(self.receipt.channel)?;
        let prerelease = false;
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
