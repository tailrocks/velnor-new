//! Actual caller contexts for publication recovery and read-only tap continuation.
use super::super::{
    github::GitHubApi,
    run_identity::{RunSource, run_matches},
};
use crate::{OrchestratorError, internal::internal};
use velnor_actions_contract::{PackageReleasePlan, package_release::PackageReleaseChannel};

pub(in crate::package_release_publish) const CONTINUATION_PATH: &str =
    ".github/workflows/preview-tap-reconcile.yml";
pub(in crate::package_release_publish) const CONTINUATION_NAME: &str = "Preview tap reconciliation";

#[derive(Clone, Copy)]
pub(in crate::package_release_publish) enum Purpose {
    PublicationRecovery,
    TapPromotion,
}

pub(in crate::package_release_publish) struct Invocation {
    pub(in crate::package_release_publish) repository: String,
    pub(in crate::package_release_publish) source_ref: String,
    pub(in crate::package_release_publish) source_sha: String,
    pub(in crate::package_release_publish) workflow_ref: String,
    pub(in crate::package_release_publish) workflow_sha: String,
    pub(in crate::package_release_publish) event: String,
}

impl Invocation {
    pub(in crate::package_release_publish) fn from_env() -> Result<Self, OrchestratorError> {
        Ok(Self {
            repository: super::env_text("GITHUB_REPOSITORY")?,
            source_ref: super::env_text("GITHUB_REF")?,
            source_sha: super::env_text("GITHUB_SHA")?,
            workflow_ref: super::env_text("GITHUB_WORKFLOW_REF")?,
            workflow_sha: super::env_text("GITHUB_WORKFLOW_SHA")?,
            event: super::env_text("GITHUB_EVENT_NAME")?,
        })
    }

    pub(in crate::package_release_publish) fn continuation(
        &self,
        plan: &PackageReleasePlan,
    ) -> bool {
        matches!(self.event.as_str(), "schedule" | "workflow_dispatch")
            && self.workflow_ref == Self::expected_workflow(plan, CONTINUATION_PATH)
    }

    pub(in crate::package_release_publish) fn validate(
        &self,
        plan: &PackageReleasePlan,
        purpose: Purpose,
    ) -> Result<(), OrchestratorError> {
        let manual = self.event == "workflow_dispatch"
            && self.workflow_ref == Self::expected_workflow(plan, plan.channel.workflow_path());
        let permitted =
            manual || matches!(purpose, Purpose::TapPromotion) && self.continuation(plan);
        if plan.channel != PackageReleaseChannel::Preview
            || !permitted
            || self.repository != plan.repository
            || self.source_ref != format!("refs/heads/{}", plan.default_branch)
            || !super::is_git_sha(&self.source_sha)
            || self.workflow_sha != self.source_sha
        {
            return Err(internal("package_recovery_context_mismatch"));
        }
        Ok(())
    }

    pub(in crate::package_release_publish) fn authenticate_continuation(
        &self,
        plan: &PackageReleasePlan,
        api: &GitHubApi,
        repo_id: u64,
    ) -> Result<(), OrchestratorError> {
        self.validate(plan, Purpose::TapPromotion)?;
        if !self.continuation(plan) {
            return Err(internal("tap_continuation_context"));
        }
        let workflow = api.admitted_workflow(CONTINUATION_PATH, CONTINUATION_NAME)?;
        let id = super::positive_env("GITHUB_RUN_ID")?;
        let attempt = super::positive_env_u32("GITHUB_RUN_ATTEMPT")?;
        for run in [
            api.workflow_run(id, attempt)?,
            api.get_json(&api.route(&format!("actions/runs/{id}")))?,
        ] {
            if !run_matches(
                &run,
                &workflow,
                id,
                attempt,
                RunSource {
                    repository_id: repo_id,
                    sha: &self.source_sha,
                    branch: &plan.default_branch,
                },
                &self.event,
            ) || run.status.as_deref() != Some("in_progress")
                || run.conclusion.is_some()
            {
                return Err(internal("tap_continuation_current_run"));
            }
        }
        Ok(())
    }

    fn expected_workflow(plan: &PackageReleasePlan, path: &str) -> String {
        format!(
            "{}/{path}@refs/heads/{}",
            plan.repository, plan.default_branch
        )
    }
}

#[cfg(test)]
#[path = "package_release_promotion_context_tests.rs"]
mod tests;
