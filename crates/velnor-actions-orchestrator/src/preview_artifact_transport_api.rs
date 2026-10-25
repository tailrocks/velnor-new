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

    fn authenticate_with_event(
        &self,
        plan: &PackageReleasePlan,
        context: &ReleaseProductionContext,
        consumer: ReleaseConsumer,
        event: &str,
    ) -> Result<(), OrchestratorError> {
        self.authenticate_with_event_and_ancestry(
            plan,
            context,
            consumer,
            event,
            |api, repository_id, branch, source, authority| {
                api.verify_preview_main_ancestry(repository_id, branch, source, authority)
                    .map(|_| ())
            },
        )
    }

    fn authenticate_with_event_and_ancestry(
        &self,
        plan: &PackageReleasePlan,
        context: &ReleaseProductionContext,
        consumer: ReleaseConsumer,
        event: &str,
        verify_ancestry: impl FnOnce(&GitHubApi, u64, &str, &str, &str) -> Result<(), OrchestratorError>,
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
        if !matches!(event, "workflow_run" | "workflow_dispatch") {
            return Err(internal("preview_artifact_current_run"));
        }
        let workflow = api.admitted_workflow(channel.workflow_path(), channel.workflow_name())?;
        let authority = RunSource {
            repository_id: context.repository_id,
            sha: &context.workflow_authority_sha,
            branch: &plan.default_branch,
        };
        if channel == PackageReleaseChannel::Preview {
            verify_ancestry(
                api,
                context.repository_id,
                &plan.default_branch,
                &context.source_sha,
                &context.workflow_authority_sha,
            )?;
        }
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
}

impl ArtifactReadApi for ReleaseArtifactApi {
    fn authenticate(
        &self,
        plan: &PackageReleasePlan,
        context: &ReleaseProductionContext,
        consumer: ReleaseConsumer,
    ) -> Result<(), OrchestratorError> {
        let event = std::env::var("GITHUB_EVENT_NAME")
            .ok()
            .filter(|event| matches!(event.as_str(), "workflow_run" | "workflow_dispatch"))
            .ok_or_else(|| internal("preview_artifact_current_run"))?;
        self.authenticate_with_event(plan, context, consumer, &event)
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
