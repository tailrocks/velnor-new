//! Independent native reauthentication of producing workflow and upstream CI.

use super::github::{
    GitHubApi, WorkflowJobList, WorkflowMetadata, WorkflowRunList, WorkflowRunResponse,
    encode_segment,
};
use crate::{OrchestratorError, internal::internal};
use std::time::Duration;
use velnor_actions_contract::{
    CI_WORKFLOW_PATH, REQUIRED_DISPLAY_NAME, WORKFLOW_DISPLAY_NAME,
    package_release::PackageReleaseReceipt,
};

#[path = "package_release_active_job.rs"]
mod active_job;
#[path = "package_release_run_identity_cache.rs"]
mod cache;
pub(super) use active_job::CurrentRun;
#[path = "package_release_active_role.rs"]
mod active_role;
pub(super) use active_role::{continuation_job_name, release_job_name};
pub(super) use cache::RunIdentityCache;

#[path = "package_release_run_inventory.rs"]
mod inventory;
pub(super) use inventory::sleep_for_retry;
use inventory::{
    WORKFLOW_RUN_PAGE_SIZE, check_retry_deadline, latest_ci_run, retry_empty_workflow_run_inventory,
};

#[path = "package_release_run_jobs.rs"]
mod jobs;
use jobs::{
    MAX_WORKFLOW_JOB_PAGES, WORKFLOW_JOBS_PAGE_SIZE, WorkflowJobsAccumulator, WorkflowJobsPage,
};

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
        if retained {
            // A retained signed receipt is bound to its original attempt. A later
            // rerun changes the aggregate run endpoint, but does not revoke that
            // completed attempt; authenticate the exact attempt endpoint instead.
            return self.verify_release_run(receipt, true);
        }
        let run: WorkflowRunResponse =
            self.get_json(&self.route(&format!("actions/runs/{}", receipt.workflow.run_id)))?;
        let identity = CurrentRun {
            id: receipt.workflow.run_id,
            attempt: receipt.workflow.run_attempt,
            source: RunSource {
                repository_id: receipt.repository.id,
                sha: &receipt.workflow.authority_sha,
                branch: &receipt.source.default_branch,
            },
            workflow: &workflow,
            event: receipt.channel.event().as_str(),
        };
        if !run_matches(
            &run,
            &workflow,
            identity.id,
            identity.attempt,
            identity.source,
            identity.event,
        ) || !matches!(run.status.as_deref(), Some("queued" | "in_progress"))
            || run.conclusion.is_some()
        {
            return Err(internal("package_original_run_identity_mismatch"));
        }
        self.require_active_job(&identity, "Publish authenticated channel release")
    }

    fn verify_release_run_response(
        receipt: &PackageReleaseReceipt,
        retained: bool,
        run: &WorkflowRunResponse,
        workflow: &WorkflowMetadata,
    ) -> Result<(), OrchestratorError> {
        let source = RunSource {
            repository_id: receipt.repository.id,
            sha: &receipt.workflow.authority_sha,
            branch: &receipt.source.default_branch,
        };
        if !run_matches(
            run,
            workflow,
            receipt.workflow.run_id,
            receipt.workflow.run_attempt,
            source,
            receipt.channel.event().as_str(),
        ) || if retained {
            run.status.as_deref() != Some("completed")
        } else {
            !matches!(run.status.as_deref(), Some("queued" | "in_progress"))
                || run.conclusion.is_some()
        } {
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
            if !run_repository_names_match(&run, self.repository_name())
                || !ci_run_is_success(&run, id, attempt, source, &workflow)
            {
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
            if !run_repository_names_match(&current, self.repository_name())
                || !ci_run_is_success(&current, id, attempt, source, &workflow)
            {
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
        self.latest_source_ci_with_sleep(workflow, source, sleep_for_retry)
    }

    pub(super) fn latest_source_ci_with_sleep(
        &self,
        workflow: &WorkflowMetadata,
        source: RunSource<'_>,
        sleep: impl FnMut(Duration) -> Result<(), OrchestratorError>,
    ) -> Result<WorkflowRunResponse, OrchestratorError> {
        self.latest_source_ci_optional_with_sleep(workflow, source, sleep)?
            .ok_or_else(|| internal("package_ci_run_missing"))
    }

    pub(super) fn latest_source_ci_optional_with_sleep(
        &self,
        workflow: &WorkflowMetadata,
        source: RunSource<'_>,
        sleep: impl FnMut(Duration) -> Result<(), OrchestratorError>,
    ) -> Result<Option<WorkflowRunResponse>, OrchestratorError> {
        let Some(runs) = self.source_ci_run_inventory(workflow, source, sleep)? else {
            return Ok(None);
        };
        latest_ci_run(runs, workflow, source, self.repository_name()).map(Some)
    }

    fn source_ci_run_inventory(
        &self,
        workflow: &WorkflowMetadata,
        source: RunSource<'_>,
        sleep: impl FnMut(Duration) -> Result<(), OrchestratorError>,
    ) -> Result<Option<Vec<WorkflowRunResponse>>, OrchestratorError> {
        retry_empty_workflow_run_inventory(
            |page| {
                let list: WorkflowRunList = self.get_json(&self.route(&format!(
                    "actions/workflows/{}/runs?head_sha={}&per_page={WORKFLOW_RUN_PAGE_SIZE}&page={page}",
                    workflow.id,
                    encode_segment(source.sha),
                )))?;
                Ok(list.workflow_runs)
            },
            sleep,
            check_retry_deadline,
        )
    }

    pub(super) fn complete_workflow_jobs(
        &self,
        id: u64,
        attempt: u32,
    ) -> Result<WorkflowJobList, OrchestratorError> {
        let mut jobs = WorkflowJobsAccumulator::default();
        for page in 1..=MAX_WORKFLOW_JOB_PAGES {
            let response: WorkflowJobsPage = self.get_json(&self.route(&format!(
                "actions/runs/{id}/attempts/{attempt}/jobs?per_page={WORKFLOW_JOBS_PAGE_SIZE}&page={page}"
            )))?;
            if jobs.admit_page(response)? {
                return Ok(jobs.into_list());
            }
        }
        Err(internal("package_ci_job_pages_exceeded"))
    }
}

fn run_repository_names_match(run: &WorkflowRunResponse, repository: &str) -> bool {
    [run.repository.as_ref(), run.head_repository.as_ref()]
        .into_iter()
        .flatten()
        .all(|run_repository| {
            run_repository
                .full_name
                .as_deref()
                .is_none_or(|full_name| full_name == repository)
        })
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

#[cfg(test)]
#[path = "package_release_run_retry_tests.rs"]
mod retry_tests;
