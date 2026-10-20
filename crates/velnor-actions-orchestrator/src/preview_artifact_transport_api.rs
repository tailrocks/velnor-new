//! Read-only capability over the existing pinned GitHub transport.

use super::super::{
    preview_artifacts::{ArtifactPage, ArtifactReadApi, ReleaseConsumer},
    run_identity::{CurrentRun, RunSource, run_matches},
};
use super::{GitHubApi, GitRefResponse, RepositoryResponse, WorkflowRunResponse};
use crate::{OrchestratorError, internal::internal};
use velnor_actions_contract::{
    PackageReleasePlan, package_release::PackageReleaseChannel,
    preview_production::ReleaseProductionContext,
};

/// No mutation method or underlying publisher API escapes this capability.
pub(in crate::package_release_publish) struct ReleaseArtifactApi {
    api: GitHubApi,
    repository: String,
}

impl ReleaseArtifactApi {
    pub(in crate::package_release_publish) fn new(repository: &str) -> Self {
        Self {
            api: GitHubApi::new(repository.into()),
            repository: repository.into(),
        }
    }
}

impl ArtifactReadApi for ReleaseArtifactApi {
    fn authenticate(
        &self,
        plan: &PackageReleasePlan,
        context: &ReleaseProductionContext,
        consumer: ReleaseConsumer,
    ) -> Result<(), OrchestratorError> {
        let api = &self.api;
        if self.repository != plan.repository {
            return Err(internal("preview_artifact_repository"));
        }
        let repository: RepositoryResponse = api.get_json(&format!("repos/{}", self.repository))?;
        if repository.id != context.repository_id
            || repository.full_name.as_deref() != Some(self.repository.as_str())
        {
            return Err(internal("preview_artifact_repository"));
        }
        let channel = plan.channel;
        let workflow = api.admitted_workflow(channel.workflow_path(), channel.workflow_name())?;
        let source = RunSource {
            repository_id: context.repository_id,
            sha: &context.source_sha,
            branch: &plan.default_branch,
        };
        // Historical attempt identity alone cannot establish the current attempt.
        for run in [
            api.workflow_run(context.run_id, context.run_attempt)?,
            api.get_json::<WorkflowRunResponse>(
                &api.route(&format!("actions/runs/{}", context.run_id)),
            )?,
        ] {
            if !run_matches(
                &run,
                &workflow,
                context.run_id,
                context.run_attempt,
                source,
                channel.event().as_str(),
            ) || !matches!(run.status.as_deref(), Some("queued" | "in_progress"))
                || run.conclusion.is_some()
            {
                return Err(internal("preview_artifact_current_run"));
            }
        }
        api.require_active_job(
            &CurrentRun {
                id: context.run_id,
                attempt: context.run_attempt,
                source,
                workflow: &workflow,
                event: channel.event().as_str(),
            },
            consumer.job_name(channel),
        )?;
        if channel == PackageReleaseChannel::Preview {
            let head: GitRefResponse = api.get_json(&api.route(&format!(
                "git/ref/heads/{}",
                super::encode_segment(&plan.default_branch)
            )))?;
            if head.ref_name != context.source_ref
                || head.object.object_type != "commit"
                || head.object.sha != context.source_sha
            {
                return Err(internal("preview_artifact_live_source"));
            }
        }
        api.verify_ci_identity(
            context.ci_run_id,
            context.ci_run_attempt,
            source,
            channel == PackageReleaseChannel::Preview,
        )
    }

    fn page(&self, run: u64, page: usize) -> Result<ArtifactPage, OrchestratorError> {
        self.api.get_json(&self.api.route(&format!(
            "actions/runs/{run}/artifacts?per_page=100&page={page}"
        )))
    }

    fn download(
        &self,
        id: u64,
        size: u64,
        output: &mut std::fs::File,
        deadline: velnor_actions_mise::CheckDeadline,
    ) -> Result<(), OrchestratorError> {
        self.api.request_to_file(
            self.api.route(&format!("actions/artifacts/{id}/zip")),
            output,
            size,
            deadline,
        )
    }
}

#[cfg(test)]
#[path = "preview_artifact_transport_api_tests.rs"]
mod tests;
