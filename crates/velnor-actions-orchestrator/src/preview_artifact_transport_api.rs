//! Read-only capability over the existing pinned GitHub transport.

use super::super::{
    preview_artifacts::{ArtifactPage, ArtifactReadApi, JobRecord, ReleaseConsumer},
    run_identity::{CurrentRun, RunSource, run_matches},
};
use super::{GitHubApi, RepositoryResponse, WorkflowRunResponse};
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

    #[cfg(test)]
    pub(super) fn authenticate_with_event(
        &self,
        plan: &PackageReleasePlan,
        context: &ReleaseProductionContext,
        consumer: ReleaseConsumer,
        event: &str,
    ) -> Result<(), OrchestratorError> {
        let event = supported_release_event(Some(event))?;
        authenticate_release_caller(
            &self.api,
            &self.repository,
            plan,
            context,
            consumer,
            event,
            || Ok(()),
        )
    }
}

fn supported_release_event(event: Option<&str>) -> Result<&str, OrchestratorError> {
    event
        .filter(|event| matches!(*event, "workflow_run" | "workflow_dispatch"))
        .ok_or_else(|| internal("preview_artifact_current_run"))
}

impl ArtifactReadApi for ReleaseArtifactApi {
    fn authenticate(
        &self,
        plan: &PackageReleasePlan,
        context: &ReleaseProductionContext,
        consumer: ReleaseConsumer,
    ) -> Result<(), OrchestratorError> {
        let event = std::env::var("GITHUB_EVENT_NAME").ok();
        let event = supported_release_event(event.as_deref())?;
        authenticate_release_caller(
            &self.api,
            &self.repository,
            plan,
            context,
            consumer,
            event,
            || {
                self.api
                    .verify_preview_main_ancestry(
                        context.repository_id,
                        &plan.default_branch,
                        &context.source_sha,
                        &context.workflow_authority_sha,
                    )
                    .map(|_| ())
            },
        )
    }

    fn page(&self, run: u64, page: usize) -> Result<ArtifactPage, OrchestratorError> {
        self.api.get_json(&self.api.route(&format!(
            "actions/runs/{run}/artifacts?per_page=100&page={page}"
        )))
    }

    fn jobs(&self, run: u64, attempt: u32) -> Result<Vec<JobRecord>, OrchestratorError> {
        let response = self.api.complete_workflow_jobs(run, attempt)?;
        Ok(response
            .jobs
            .into_iter()
            .map(|job| JobRecord {
                run_id: job.run_id,
                name: job.name,
                head_sha: job.head_sha,
                status: job.status,
                conclusion: job.conclusion,
            })
            .collect())
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

fn authenticate_release_caller(
    api: &GitHubApi,
    repository_name: &str,
    plan: &PackageReleasePlan,
    context: &ReleaseProductionContext,
    consumer: ReleaseConsumer,
    event: &str,
    verify_preview_ancestry: impl FnOnce() -> Result<(), OrchestratorError>,
) -> Result<(), OrchestratorError> {
    if repository_name != plan.repository {
        return Err(internal("preview_artifact_repository"));
    }
    let repository: RepositoryResponse = api.get_json(&format!("repos/{repository_name}"))?;
    if repository.id != context.repository_id
        || repository.full_name.as_deref() != Some(repository_name)
    {
        return Err(internal("preview_artifact_repository"));
    }
    let channel = plan.channel;
    let workflow = api.admitted_workflow(channel.workflow_path(), channel.workflow_name())?;
    let authority = RunSource {
        repository_id: context.repository_id,
        sha: &context.workflow_authority_sha,
        branch: &plan.default_branch,
    };
    if channel == PackageReleaseChannel::Preview {
        verify_preview_ancestry()?;
    }
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
            authority,
            event,
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
            source: authority,
            workflow: &workflow,
            event,
        },
        consumer.job_name(channel),
    )?;
    let ci_source = RunSource {
        repository_id: context.repository_id,
        sha: &context.source_sha,
        branch: &plan.default_branch,
    };
    api.verify_ci_identity(
        context.ci_run_id,
        context.ci_run_attempt,
        ci_source,
        channel == PackageReleaseChannel::Preview,
    )
}

#[cfg(test)]
#[path = "preview_artifact_transport_api_tests.rs"]
mod tests;
