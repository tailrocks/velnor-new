//! Independent native reauthentication of producing workflow and upstream CI.

use super::github::{
    GitHubApi, WorkflowJobList, WorkflowMetadata, WorkflowRunList, WorkflowRunResponse,
    encode_segment,
};
use crate::{OrchestratorError, internal::internal};
use velnor_actions_contract::{
    CI_WORKFLOW_PATH, REQUIRED_DISPLAY_NAME, WORKFLOW_DISPLAY_NAME,
    package_release::PackageReleaseReceipt,
};

#[path = "package_release_run_identity_cache.rs"]
mod cache;
pub(super) use cache::RunIdentityCache;

#[derive(Clone, Copy)]
pub(super) struct RunSource<'a> {
    pub repository_id: u64,
    pub sha: &'a str,
    pub branch: &'a str,
}

impl GitHubApi {
    pub(super) fn workflow_run(
        &self,
        run_id: u64,
        attempt: u32,
    ) -> Result<WorkflowRunResponse, OrchestratorError> {
        self.get_json(&self.route(&format!("actions/runs/{run_id}/attempts/{attempt}")))
    }

    pub(super) fn admitted_workflow(
        &self,
        path: &str,
        name: &str,
    ) -> Result<WorkflowMetadata, OrchestratorError> {
        if let Some(workflow) = self.run_identity_cache.borrow().workflow(path, name) {
            return Ok(workflow);
        }
        let filename = path
            .rsplit('/')
            .next()
            .ok_or_else(|| internal("package_workflow_path"))?;
        let workflow: WorkflowMetadata =
            self.get_json(&self.route(&format!("actions/workflows/{}", encode_segment(filename))))?;
        validate_workflow_metadata(&workflow, path, name)?;
        self.run_identity_cache
            .borrow_mut()
            .admit_workflow(&workflow)?;
        Ok(workflow)
    }

    pub(super) fn verify_release_run(
        &self,
        receipt: &PackageReleaseReceipt,
        retained: bool,
    ) -> Result<(), OrchestratorError> {
        let workflow = self.admitted_workflow(
            receipt.channel.workflow_path(),
            receipt.channel.workflow_name(),
        )?;
        let run = self.workflow_run(receipt.workflow.run_id, receipt.workflow.run_attempt)?;
        Self::verify_release_run_response(receipt, retained, &run, &workflow)
    }

    pub(super) fn verify_current_release_run(
        &self,
        receipt: &PackageReleaseReceipt,
        retained: bool,
    ) -> Result<(), OrchestratorError> {
        let workflow = self.admitted_workflow(
            receipt.channel.workflow_path(),
            receipt.channel.workflow_name(),
        )?;
        let run: WorkflowRunResponse =
            self.get_json(&self.route(&format!("actions/runs/{}", receipt.workflow.run_id)))?;
        Self::verify_release_run_response(receipt, retained, &run, &workflow)
    }

    fn verify_release_run_response(
        receipt: &PackageReleaseReceipt,
        retained: bool,
        run: &WorkflowRunResponse,
        workflow: &WorkflowMetadata,
    ) -> Result<(), OrchestratorError> {
        let source = RunSource {
            repository_id: receipt.repository.id,
            sha: &receipt.source.commit_sha,
            branch: &receipt.source.default_branch,
        };
        if !run_matches(
            run,
            workflow,
            receipt.workflow.run_id,
            receipt.workflow.run_attempt,
            source,
            receipt.channel.event().as_str(),
        ) || run.status.as_deref() != Some(if retained { "completed" } else { "in_progress" })
            || (!retained && run.conclusion.is_some())
        {
            return Err(internal("package_original_run_identity_mismatch"));
        }
        Ok(())
    }

    pub(super) fn verify_ci_identity(
        &self,
        id: u64,
        attempt: u32,
        source: RunSource<'_>,
        latest: bool,
    ) -> Result<(), OrchestratorError> {
        let workflow = self.admitted_workflow(CI_WORKFLOW_PATH, WORKFLOW_DISPLAY_NAME)?;
        let key = cache::CompletedAttempt::new(id, attempt, source, &workflow);
        if !self.run_identity_cache.borrow().contains(&key) {
            let run = self.workflow_run(id, attempt)?;
            if !ci_run_is_success(&run, id, attempt, source, &workflow) {
                return Err(internal("package_ci_run_identity_mismatch"));
            }
            require_required_job(&self.complete_workflow_jobs(id, attempt)?, id, source.sha)?;
            self.run_identity_cache.borrow_mut().admit_completed(key)?;
        }
        if latest {
            let observed = self.latest_source_ci(&workflow, source)?;
            if observed.id != Some(id)
                || observed.run_attempt != Some(attempt)
                || !ci_run_is_success(&observed, id, attempt, source, &workflow)
            {
                return Err(internal("package_ci_latest_attempt_mismatch"));
            }
            let current: WorkflowRunResponse =
                self.get_json(&self.route(&format!("actions/runs/{id}")))?;
            if !ci_run_is_success(&current, id, attempt, source, &workflow) {
                return Err(internal("package_ci_latest_attempt_mismatch"));
            }
        }
        Ok(())
    }

    pub(super) fn latest_source_ci(
        &self,
        workflow: &WorkflowMetadata,
        source: RunSource<'_>,
    ) -> Result<WorkflowRunResponse, OrchestratorError> {
        let mut runs = Vec::new();
        for page in 1..=10 {
            let list: WorkflowRunList = self.get_json(&self.route(&format!(
                "actions/workflows/{}/runs?head_sha={}&branch={}&event=push&per_page=100&page={page}",
                workflow.id, encode_segment(source.sha), encode_segment(source.branch))))?;
            let count = list.workflow_runs.len();
            runs.extend(list.workflow_runs);
            if count < 100 {
                return latest_ci_run(runs, workflow, source);
            }
        }
        Err(internal("package_ci_run_pages_exceeded"))
    }

    pub(super) fn complete_workflow_jobs(
        &self,
        id: u64,
        attempt: u32,
    ) -> Result<WorkflowJobList, OrchestratorError> {
        let mut jobs = Vec::new();
        for page in 1..=20 {
            let list: WorkflowJobList = self.get_json(&self.route(&format!(
                "actions/runs/{id}/attempts/{attempt}/jobs?per_page=100&page={page}"
            )))?;
            let count = list.jobs.len();
            jobs.extend(list.jobs);
            if count < 100 {
                return Ok(WorkflowJobList { jobs });
            }
        }
        Err(internal("package_ci_job_pages_exceeded"))
    }
}

pub(super) fn run_matches(
    run: &WorkflowRunResponse,
    workflow: &WorkflowMetadata,
    id: u64,
    attempt: u32,
    source: RunSource<'_>,
    event: &str,
) -> bool {
    run.id == Some(id)
        && run.run_attempt == Some(attempt)
        && run.workflow_id == Some(workflow.id)
        && run.path.as_deref() == Some(workflow.path.as_str())
        && run.name.as_deref() == Some(workflow.name.as_str())
        && run.head_sha.as_deref() == Some(source.sha)
        && run.head_branch.as_deref() == Some(source.branch)
        && run.event.as_deref() == Some(event)
        && run
            .repository
            .as_ref()
            .is_some_and(|repo| repo.id == source.repository_id)
        && run
            .head_repository
            .as_ref()
            .is_some_and(|repo| repo.id == source.repository_id)
}

pub(super) fn ci_run_is_success(
    run: &WorkflowRunResponse,
    id: u64,
    attempt: u32,
    source: RunSource<'_>,
    workflow: &WorkflowMetadata,
) -> bool {
    run_matches(run, workflow, id, attempt, source, "push")
        && run.status.as_deref() == Some("completed")
        && run.conclusion.as_deref() == Some("success")
}

fn latest_ci_run(
    runs: Vec<WorkflowRunResponse>,
    workflow: &WorkflowMetadata,
    source: RunSource<'_>,
) -> Result<WorkflowRunResponse, OrchestratorError> {
    if runs.is_empty()
        || runs.iter().any(|run| {
            run.run_number.is_none_or(|n| n == 0)
                || !run_matches(
                    run,
                    workflow,
                    run.id.unwrap_or(0),
                    run.run_attempt.unwrap_or(0),
                    source,
                    "push",
                )
                || run.id.is_none_or(|id| id == 0)
                || run.run_attempt.is_none_or(|attempt| attempt == 0)
        })
    {
        return Err(internal("package_ci_run_inventory_invalid"));
    }
    let mut ordered = runs;
    ordered.sort_by_key(|run| (run.run_number, run.id));
    let ids: std::collections::BTreeSet<_> = ordered.iter().map(|run| run.id).collect();
    if ids.len() != ordered.len()
        || ordered
            .windows(2)
            .any(|pair| pair[0].run_number == pair[1].run_number)
    {
        return Err(internal("package_ci_run_inventory_ambiguous"));
    }
    ordered
        .pop()
        .ok_or_else(|| internal("package_ci_run_missing"))
}

fn validate_workflow_metadata(
    workflow: &WorkflowMetadata,
    path: &str,
    name: &str,
) -> Result<(), OrchestratorError> {
    if workflow.id == 0
        || workflow.path != path
        || workflow.name != name
        || workflow.state != "active"
    {
        return Err(internal("package_workflow_identity_mismatch"));
    }
    Ok(())
}

fn require_required_job(
    jobs: &WorkflowJobList,
    id: u64,
    sha: &str,
) -> Result<(), OrchestratorError> {
    let required: Vec<_> = jobs
        .jobs
        .iter()
        .filter(|job| job.name == REQUIRED_DISPLAY_NAME)
        .collect();
    if required.len() != 1
        || required[0].run_id != id
        || required[0].head_sha.as_deref() != Some(sha)
        || required[0].status.as_deref() != Some("completed")
        || required[0].conclusion.as_deref() != Some("success")
    {
        return Err(internal("package_ci_required_job_invalid"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "package_release_run_identity_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "package_release_run_transport_tests.rs"]
mod transport_tests;
