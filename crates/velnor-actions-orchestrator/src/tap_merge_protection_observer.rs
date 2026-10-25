use super::{
    candidate::MergeCandidate,
    github::TapMergeGitHub,
    observation::{MergeAuthority, required_numeric, required_string, valid_app_slug},
};
use crate::{OrchestratorError, internal::internal};
use serde_json::Value;
use std::time::Instant;

pub(super) trait ClassicProtectionReader {
    fn classic_protection(
        &self,
        repository: &str,
        encoded_branch: &str,
    ) -> Result<Option<Value>, OrchestratorError>;
}

pub(super) struct ProtectionObserver {
    api: TapMergeGitHub,
    repository: String,
    encoded_branch: String,
}

impl ProtectionObserver {
    pub(super) fn authenticate(
        candidate: &MergeCandidate,
        authority: &MergeAuthority,
        token: String,
        deadline: Instant,
    ) -> Result<Self, OrchestratorError> {
        let app_id = required_numeric("VELNOR_TAP_PROTECTION_APP_ID")?;
        let slug = required_string("VELNOR_TAP_PROTECTION_APP_SLUG")?;
        let installation = required_numeric("VELNOR_TAP_PROTECTION_INSTALLATION_ID")?;
        validate_identity(
            app_id,
            &slug,
            installation,
            authority.writer_app_id,
            required_numeric("VELNOR_TAP_INSTALLATION_ID")?,
        )?;
        let api = TapMergeGitHub::with_deadline(token, deadline);
        authenticate_scope(
            &api,
            &candidate.policy.repository,
            authority.repo_id,
            app_id,
            &slug,
        )?;
        Ok(Self {
            api,
            repository: candidate.policy.repository.clone(),
            encoded_branch: super::observation::encode_segment(&candidate.plan.default_branch),
        })
    }
}

impl ClassicProtectionReader for ProtectionObserver {
    fn classic_protection(
        &self,
        repository: &str,
        encoded_branch: &str,
    ) -> Result<Option<Value>, OrchestratorError> {
        if repository != self.repository || encoded_branch != self.encoded_branch {
            return Err(internal("tap_merge_observer_scope"));
        }
        self.api.get_optional(&format!(
            "repos/{repository}/branches/{encoded_branch}/protection"
        ))
    }
}

fn validate_identity(
    app_id: u64,
    slug: &str,
    installation: u64,
    writer_app: u64,
    writer_installation: u64,
) -> Result<(), OrchestratorError> {
    if app_id == 0
        || installation == 0
        || app_id == writer_app
        || app_id == velnor_actions_contract::homebrew_review_completion::GITHUB_ACTIONS_APP_ID
        || installation == writer_installation
        || !valid_app_slug(slug)
    {
        return Err(internal("tap_merge_observer_identity"));
    }
    Ok(())
}

fn authenticate_scope(
    api: &TapMergeGitHub,
    repository: &str,
    repo_id: u64,
    app_id: u64,
    slug: &str,
) -> Result<(), OrchestratorError> {
    let app: Value = api.get(&format!("apps/{slug}"))?;
    if app["id"].as_u64() != Some(app_id) || app["slug"].as_str() != Some(slug) {
        return Err(internal("tap_merge_observer_app_identity"));
    }
    let repo: Value = api.get(&format!("repos/{repository}"))?;
    let installation: Value = api.get("installation/repositories?per_page=100&page=1")?;
    let repos = installation["repositories"]
        .as_array()
        .ok_or_else(|| internal("tap_merge_observer_installation_scope"))?;
    if repo["id"].as_u64() != Some(repo_id)
        || repo["full_name"].as_str() != Some(repository)
        || installation["total_count"].as_u64() != Some(1)
        || repos.len() != 1
        || repos[0]["id"].as_u64() != Some(repo_id)
        || repos[0]["full_name"].as_str() != Some(repository)
    {
        return Err(internal("tap_merge_observer_installation_scope"));
    }
    Ok(())
}

pub(super) fn validate_distinct_tokens(
    observer: &str,
    writer: &str,
    source: &str,
) -> Result<(), OrchestratorError> {
    if observer.is_empty()
        || observer == writer
        || observer == source
        || std::env::var("VELNOR_TAP_TOKEN").is_ok_and(|token| token == observer)
    {
        return Err(internal("tap_merge_observer_credentials_reused"));
    }
    Ok(())
}
