//! Current job authority is distinct from the workflow's aggregate queue status.

use super::super::github::{GitHubApi, WorkflowRunResponse};
use super::{RunSource, WorkflowMetadata, run_matches};
use crate::{OrchestratorError, internal::internal};
use serde::Deserialize;
use std::collections::BTreeSet;

pub(in crate::package_release_publish) struct CurrentRun<'a> {
    pub id: u64,
    pub attempt: u32,
    pub source: RunSource<'a>,
    pub workflow: &'a WorkflowMetadata,
    pub event: &'a str,
}

#[derive(Deserialize)]
struct JobPage {
    total_count: usize,
    jobs: Vec<ActiveJob>,
}
#[derive(Deserialize)]
struct ActiveJob {
    id: u64,
    run_id: u64,
    name: String,
    head_sha: Option<String>,
    status: String,
    conclusion: Option<String>,
    started_at: Option<String>,
}

impl GitHubApi {
    pub(in crate::package_release_publish) fn verify_current_role(
        &self,
        identity: &CurrentRun<'_>,
        name: &str,
    ) -> Result<(), OrchestratorError> {
        for run in [
            self.workflow_run(identity.id, identity.attempt)?,
            self.get_json::<WorkflowRunResponse>(
                &self.route(&format!("actions/runs/{}", identity.id)),
            )?,
        ] {
            if !run_matches(
                &run,
                identity.workflow,
                identity.id,
                identity.attempt,
                identity.source,
                identity.event,
            ) || !matches!(run.status.as_deref(), Some("queued" | "in_progress"))
                || run.conclusion.is_some()
            {
                return Err(internal("package_current_role_run"));
            }
        }
        self.require_active_job(identity, name)
    }

    pub(in crate::package_release_publish) fn require_active_job(
        &self,
        identity: &CurrentRun<'_>,
        name: &str,
    ) -> Result<(), OrchestratorError> {
        let key = super::cache::CompletedAttempt::new(
            identity.id,
            identity.attempt,
            identity.source,
            identity.workflow,
        );
        let cached = self.run_identity_cache.borrow().active_job(&key, name);
        if let Some(id) = cached {
            let job: ActiveJob = self.get_json(&self.route(&format!("actions/jobs/{id}")))?;
            return validate_job(&job, identity, name, Some(id));
        }
        let mut total = None;
        let mut ids = BTreeSet::new();
        let mut selected = Vec::new();
        for page in 1..=20 {
            let response: JobPage = self.get_json(&self.route(&format!(
                "actions/runs/{}/attempts/{}/jobs?per_page=100&page={page}",
                identity.id, identity.attempt
            )))?;
            if response.total_count > 2000 || total.is_some_and(|n| n != response.total_count) {
                return Err(internal("package_active_job_inventory"));
            }
            total = Some(response.total_count);
            let remaining = response
                .total_count
                .checked_sub(ids.len())
                .ok_or_else(|| internal("package_active_job_inventory"))?;
            if response.jobs.len() != remaining.min(100) {
                return Err(internal("package_active_job_inventory"));
            }
            for job in response.jobs {
                if job.id == 0 || !ids.insert(job.id) {
                    return Err(internal("package_active_job_inventory"));
                }
                if job.name == name {
                    selected.push(job);
                }
            }
            if ids.len() == response.total_count {
                if selected.len() != 1 {
                    return Err(internal("package_active_job_identity"));
                }
                let job = &selected[0];
                validate_job(job, identity, name, None)?;
                return self
                    .run_identity_cache
                    .borrow_mut()
                    .admit_active_job(key, name, job.id);
            }
        }
        Err(internal("package_active_job_inventory"))
    }
}

fn validate_job(
    job: &ActiveJob,
    identity: &CurrentRun<'_>,
    name: &str,
    id: Option<u64>,
) -> Result<(), OrchestratorError> {
    if job.id == 0
        || id.is_some_and(|id| id != job.id)
        || job.run_id != identity.id
        || job.name != name
        || job.head_sha.as_deref() != Some(identity.source.sha)
        || job.status != "in_progress"
        || job.conclusion.is_some()
        || job.started_at.as_ref().is_none_or(String::is_empty)
    {
        return Err(internal("package_active_job_identity"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "package_release_active_job_tests.rs"]
mod tests;
