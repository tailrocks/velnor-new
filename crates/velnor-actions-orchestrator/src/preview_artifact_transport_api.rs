//! Read-only capability over the existing pinned GitHub transport.

use super::super::{
    preview_artifacts::{ArtifactPage, ArtifactReadApi, PreviewConsumer},
    run_identity::{RunSource, run_matches},
};
use super::{GitHubApi, GitRefResponse, RepositoryResponse, WorkflowRunResponse};
use crate::{OrchestratorError, internal::internal};
use velnor_actions_contract::{
    PackageReleasePlan, package_release::PackageReleaseChannel,
    preview_production::PreviewProductionContext,
};

/// No mutation method or underlying publisher API escapes this capability.
pub(in crate::package_release_publish) struct PreviewArtifactApi {
    api: GitHubApi,
    repository: String,
}

impl PreviewArtifactApi {
    pub(in crate::package_release_publish) fn new(repository: &str) -> Self {
        Self {
            api: GitHubApi::new(repository.into()),
            repository: repository.into(),
        }
    }
}

impl ArtifactReadApi for PreviewArtifactApi {
    fn authenticate(
        &self,
        plan: &PackageReleasePlan,
        context: &PreviewProductionContext,
        consumer: PreviewConsumer,
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
        let channel = PackageReleaseChannel::Preview;
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
        self.require_active_consumer(context, consumer)?;
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
        api.verify_ci_identity(context.ci_run_id, context.ci_run_attempt, source, true)
    }

    fn page(&self, run: u64, page: usize) -> Result<ArtifactPage, OrchestratorError> {
        self.api.get_json(&self.api.route(&format!(
            "actions/runs/{run}/artifacts?per_page=100&page={page}"
        )))
    }

    fn download(&self, id: u64, size: u64) -> Result<Vec<u8>, OrchestratorError> {
        let limit = usize::try_from(size).map_err(|_| internal("preview_artifact_size"))?;
        self.api.request_accept(
            vec![std::ffi::OsString::from(
                self.api.route(&format!("actions/artifacts/{id}/zip")),
            )],
            limit,
            "application/vnd.github+json",
        )
    }
}

#[cfg(test)]
#[path = "preview_artifact_transport_api_tests.rs"]
mod tests;

#[derive(serde::Deserialize)]
struct ConsumerJobPage {
    total_count: usize,
    jobs: Vec<ConsumerJob>,
}
#[derive(serde::Deserialize)]
struct ConsumerJob {
    id: u64,
    run_id: u64,
    name: String,
    head_sha: Option<String>,
    status: String,
    conclusion: Option<String>,
}

impl PreviewArtifactApi {
    fn require_active_consumer(
        &self,
        context: &PreviewProductionContext,
        consumer: PreviewConsumer,
    ) -> Result<(), OrchestratorError> {
        let mut total = None;
        let mut ids = std::collections::BTreeSet::new();
        let mut consumers = Vec::new();
        for page in 1..=20 {
            let response: ConsumerJobPage = self.api.get_json(&self.api.route(&format!(
                "actions/runs/{}/attempts/{}/jobs?per_page=100&page={page}",
                context.run_id, context.run_attempt
            )))?;
            if response.total_count > 2000 || total.is_some_and(|n| n != response.total_count) {
                return Err(internal("preview_artifact_job_inventory"));
            }
            total = Some(response.total_count);
            let remaining = response
                .total_count
                .checked_sub(ids.len())
                .ok_or_else(|| internal("preview_artifact_job_inventory"))?;
            if response.jobs.len() != remaining.min(100) {
                return Err(internal("preview_artifact_job_inventory"));
            }
            for job in response.jobs {
                if job.id == 0 || !ids.insert(job.id) {
                    return Err(internal("preview_artifact_job_inventory"));
                }
                if job.name == consumer.job_name() {
                    consumers.push(job);
                }
            }
            if ids.len() == response.total_count {
                if consumers.len() != 1
                    || consumers[0].run_id != context.run_id
                    || consumers[0].head_sha.as_deref() != Some(context.source_sha.as_str())
                    || consumers[0].status != "in_progress"
                    || consumers[0].conclusion.is_some()
                {
                    return Err(internal("preview_artifact_consumer_job"));
                }
                return Ok(());
            }
        }
        Err(internal("preview_artifact_job_inventory"))
    }
}
