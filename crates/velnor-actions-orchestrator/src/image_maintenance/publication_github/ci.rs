// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_actions_mise::CheckDeadline;

use crate::{OrchestratorError, internal::internal};

use super::{
    AuthenticatedProducer, CI_WORKFLOW, PublishImageRequest, authority, get_json,
    models::{Branch, CiJob, CiJobList, Repository, WorkflowRun, WorkflowRuns},
};

const CI_JOBS_PAGE_SIZE: usize = 100;
const MAX_CI_JOB_PAGES: usize = 20;
const MAX_CI_JOBS: usize = CI_JOBS_PAGE_SIZE * MAX_CI_JOB_PAGES;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct CiIdentity {
    pub(super) id: u64,
    pub(super) attempt: u32,
}

pub(super) fn main_sha(token: &str, deadline: CheckDeadline) -> Result<String, OrchestratorError> {
    let repository: Repository = get_json("repos/tailrocks/velnor-new", token, deadline)?;
    if repository.id != authority::IMAGE_REPOSITORY_ID
        || repository.full_name != authority::IMAGE_REPOSITORY
        || repository.default_branch != "main"
    {
        return Err(internal("image_publication_repository_authority"));
    }
    let branch: Branch = get_json("repos/tailrocks/velnor-new/branches/main", token, deadline)?;
    if branch.name != "main" || !super::is_sha1(&branch.commit.sha) {
        return Err(internal("image_publication_default_branch"));
    }
    Ok(branch.commit.sha)
}

pub(super) fn authenticate(
    request: &PublishImageRequest,
    token: &str,
    deadline: CheckDeadline,
) -> Result<CiIdentity, OrchestratorError> {
    let identity = latest_successful_ci(request, token, deadline)?;
    validate_required_ci_jobs(request, token, identity, deadline)?;
    Ok(identity)
}

pub(super) fn revalidate(
    request: &PublishImageRequest,
    producer: &AuthenticatedProducer,
    token: &str,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let before_main = main_sha(token, deadline)?;
    if before_main != producer.main_sha {
        return Err(internal("image_publication_main_changed"));
    }
    super::verify_source_on_main(request, token, &before_main, deadline)?;
    let before_ci = latest_successful_ci(request, token, deadline)?;
    ensure_original_ci(&before_ci, producer)?;
    validate_required_ci_jobs(request, token, before_ci, deadline)?;
    let after_ci = latest_successful_ci(request, token, deadline)?;
    let after_main = main_sha(token, deadline)?;
    if after_main != producer.main_sha || after_ci != before_ci {
        return Err(internal("image_publication_ci_changed"));
    }
    super::verify_source_on_main(request, token, &after_main, deadline)
}

fn ensure_original_ci(
    current: &CiIdentity,
    producer: &AuthenticatedProducer,
) -> Result<(), OrchestratorError> {
    if current.id != producer.ci_run_id || current.attempt != producer.ci_run_attempt {
        return Err(internal("image_publication_ci_changed"));
    }
    Ok(())
}

fn latest_successful_ci(
    request: &PublishImageRequest,
    token: &str,
    deadline: CheckDeadline,
) -> Result<CiIdentity, OrchestratorError> {
    let route = format!(
        "repos/tailrocks/velnor-new/actions/workflows/ci.yml/runs?branch=main&event=push&head_sha={}&per_page=100",
        request.source_commit
    );
    let response: WorkflowRuns = get_json(&route, token, deadline)?;
    let mut runs = response.workflow_runs;
    validate_ci_run_inventory(&runs, request)?;
    runs.sort_by_key(|run| (run.run_number, run.id));
    let latest = runs
        .last()
        .ok_or_else(|| internal("image_publication_ci_missing"))?;
    if runs
        .iter()
        .filter(|run| run.run_number == latest.run_number)
        .count()
        != 1
        || latest.status != "completed"
        || latest.conclusion.as_deref() != Some("success")
    {
        return Err(internal("image_publication_ci_latest_not_success"));
    }
    Ok(CiIdentity {
        id: latest.id,
        attempt: latest.run_attempt,
    })
}

fn validate_ci_run_inventory(
    runs: &[WorkflowRun],
    request: &PublishImageRequest,
) -> Result<(), OrchestratorError> {
    if runs.is_empty() {
        return Err(internal("image_publication_ci_missing"));
    }
    let mut ids = BTreeSet::new();
    if runs.iter().any(|run| {
        run.id == 0
            || run.run_number == 0
            || run.run_attempt == 0
            || !ids.insert(run.id)
            || run.path != CI_WORKFLOW
            || run.event != "push"
            || run.head_branch != "main"
            || run.head_sha != request.source_commit
            || run.head_repository.as_ref().is_none_or(|repo| {
                repo.id != authority::IMAGE_REPOSITORY_ID
                    || repo.full_name != authority::IMAGE_REPOSITORY
            })
    }) {
        return Err(internal("image_publication_ci_run_identity"));
    }
    Ok(())
}

fn validate_required_ci_jobs(
    request: &PublishImageRequest,
    token: &str,
    identity: CiIdentity,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let mut inventory = CiJobInventory::default();
    for page in 1..=MAX_CI_JOB_PAGES {
        let route = format!(
            "repos/tailrocks/velnor-new/actions/runs/{}/attempts/{}/jobs?per_page={CI_JOBS_PAGE_SIZE}&page={page}",
            identity.id, identity.attempt
        );
        let response: CiJobList = get_json(&route, token, deadline)?;
        if inventory.admit_page(response, identity, &request.source_commit)? {
            return inventory.validate_required_job();
        }
    }
    Err(internal("image_publication_ci_job_inventory"))
}

#[derive(Default)]
struct CiJobInventory {
    total_count: Option<usize>,
    ids: BTreeSet<u64>,
    jobs: Vec<CiJob>,
}

impl CiJobInventory {
    fn admit_page(
        &mut self,
        page: CiJobList,
        identity: CiIdentity,
        source_sha: &str,
    ) -> Result<bool, OrchestratorError> {
        let page_len = page.jobs.len();
        if page_len > CI_JOBS_PAGE_SIZE
            || page.total_count > MAX_CI_JOBS
            || self
                .total_count
                .is_some_and(|total| total != page.total_count)
        {
            return Err(internal("image_publication_ci_job_inventory"));
        }
        self.total_count = Some(page.total_count);
        let expected_len = page
            .total_count
            .saturating_sub(self.jobs.len())
            .min(CI_JOBS_PAGE_SIZE);
        if page_len != expected_len {
            return Err(internal("image_publication_ci_job_inventory"));
        }
        for job in page.jobs {
            self.validate_job(&job, identity, source_sha)?;
            self.jobs.push(job);
        }
        Ok(self.jobs.len() == page.total_count)
    }

    fn validate_job(
        &mut self,
        job: &CiJob,
        identity: CiIdentity,
        source_sha: &str,
    ) -> Result<(), OrchestratorError> {
        let id = job.id.filter(|id| *id != 0);
        if id.is_none_or(|id| !self.ids.insert(id))
            || job.run_id != Some(identity.id)
            || job.name.as_deref().is_none_or(str::is_empty)
            || job.head_sha.as_deref() != Some(source_sha)
            || job.status.as_deref().is_none_or(str::is_empty)
        {
            return Err(internal("image_publication_ci_job_identity"));
        }
        Ok(())
    }

    fn validate_required_job(&self) -> Result<(), OrchestratorError> {
        let mut required = self
            .jobs
            .iter()
            .filter(|job| job.name.as_deref() == Some("Required"));
        let job = required
            .next()
            .ok_or_else(|| internal("image_publication_ci_required_job"))?;
        if required.next().is_some()
            || job.status.as_deref() != Some("completed")
            || job.conclusion.as_deref() != Some("success")
        {
            return Err(internal("image_publication_ci_required_job"));
        }
        Ok(())
    }
}
