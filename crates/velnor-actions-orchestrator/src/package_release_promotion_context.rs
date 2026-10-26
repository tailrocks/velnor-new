// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::{
    github::GitHubApi,
    run_identity::{CurrentRun, RunSource, continuation_job_name, release_job_name},
};
use crate::{OrchestratorError, internal::internal};
use velnor_actions_contract::{PackageReleasePlan, package_release::PackageReleaseChannel};

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
            && self.workflow_ref
                == Self::expected_workflow(plan, plan.channel.tap_reconcile_workflow_path())
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
        if !permitted
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
        let workflow = api.admitted_workflow(
            plan.channel.tap_reconcile_workflow_path(),
            plan.channel.tap_reconcile_workflow_name(),
        )?;
        let id = super::positive_env("GITHUB_RUN_ID")?;
        let attempt = super::positive_env_u32("GITHUB_RUN_ATTEMPT")?;
        let job = continuation_job_name(plan.channel, &super::env_text("GITHUB_JOB")?)?;
        api.verify_current_role(
            &CurrentRun {
                id,
                attempt,
                source: RunSource {
                    repository_id: repo_id,
                    sha: &self.source_sha,
                    branch: &plan.default_branch,
                },
                workflow: &workflow,
                event: &self.event,
            },
            job,
        )?;
        Ok(())
    }

    pub(in crate::package_release_publish) fn authenticate_recovery(
        &self,
        plan: &PackageReleasePlan,
        api: &GitHubApi,
        repo_id: u64,
    ) -> Result<(), OrchestratorError> {
        self.validate(plan, Purpose::PublicationRecovery)?;
        let expected = format!(
            "{}-recovery",
            match plan.channel {
                PackageReleaseChannel::Preview => "preview",
                PackageReleaseChannel::Stable => "stable",
            }
        );
        if super::env_text("GITHUB_JOB")? != expected {
            return Err(internal("package_recovery_role"));
        }
        let job = release_job_name(plan, &expected)?;
        let workflow =
            api.admitted_workflow(plan.channel.workflow_path(), plan.channel.workflow_name())?;
        api.verify_current_role(
            &CurrentRun {
                id: super::positive_env("GITHUB_RUN_ID")?,
                attempt: super::positive_env_u32("GITHUB_RUN_ATTEMPT")?,
                source: RunSource {
                    repository_id: repo_id,
                    sha: &self.source_sha,
                    branch: &plan.default_branch,
                },
                workflow: &workflow,
                event: &self.event,
            },
            &job,
        )
    }

    fn expected_workflow(plan: &PackageReleasePlan, path: &str) -> String {
        format!(
            "{}/{path}@refs/heads/{}",
            plan.repository, plan.default_branch
        )
    }
}
