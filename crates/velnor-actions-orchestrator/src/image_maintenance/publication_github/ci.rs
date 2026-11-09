// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_actions_mise::CheckDeadline;

use crate::{OrchestratorError, internal::internal};

use super::{
    AuthenticatedProducer, CI_WORKFLOW, PublishImageRequest, authority, get_json,
    models::{Branch, CiJob, Repository, WorkflowRun},
    pagination,
};

const MAX_CI_RUNS: usize = 999;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::image_maintenance) struct CiIdentity {
    pub(in crate::image_maintenance) id: u64,
    pub(in crate::image_maintenance) attempt: u32,
}

pub(in crate::image_maintenance) fn main_sha(
    token: &str,
    deadline: CheckDeadline,
) -> Result<String, OrchestratorError> {
    let repository: Repository = get_json("repos/tailrocks/velnor-new", token, deadline)?;
    if repository.id != authority::IMAGE_REPOSITORY_ID
        || repository.full_name != authority::IMAGE_REPOSITORY
        || repository.default_branch != "main"
    {
        return Err(internal("image_publication_repository_authority"));
    }
    let branch: Branch = get_json("repos/tailrocks/velnor-new/branches/main", token, deadline)?;
    if branch.name != "main" || !branch.protected || !super::is_sha1(&branch.commit.sha) {
        return Err(internal("image_publication_default_branch"));
    }
    Ok(branch.commit.sha)
}

pub(super) fn authenticate(
    request: &PublishImageRequest,
    token: &str,
    deadline: CheckDeadline,
) -> Result<CiIdentity, OrchestratorError> {
    authenticate_source(&request.source_commit, token, deadline)
}

pub(in crate::image_maintenance) fn authenticate_source(
    source: &str,
    token: &str,
    deadline: CheckDeadline,
) -> Result<CiIdentity, OrchestratorError> {
    let identity = latest_successful_ci(source, token, deadline)?;
    validate_required_ci_jobs(source, token, identity, deadline)?;
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
    let before_ci = latest_successful_ci(&request.source_commit, token, deadline)?;
    ensure_original_ci(&before_ci, producer)?;
    validate_required_ci_jobs(&request.source_commit, token, before_ci, deadline)?;
    let after_ci = latest_successful_ci(&request.source_commit, token, deadline)?;
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
    source: &str,
    token: &str,
    deadline: CheckDeadline,
) -> Result<CiIdentity, OrchestratorError> {
    let route = format!(
        "repos/tailrocks/velnor-new/actions/workflows/ci.yml/runs?branch=main&event=push&head_sha={source}"
    );
    let mut runs = pagination::get_complete(
        &pagination::Collection {
            route: &route,
            name: "workflow_runs",
            max_items: MAX_CI_RUNS,
            error: "image_publication_ci_run_inventory",
            identity_error: "image_publication_ci_run_identity",
            id: |run: &WorkflowRun| Some(run.id),
        },
        token,
        deadline,
    )?;
    validate_ci_run_inventory(&runs, source)?;
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

fn validate_ci_run_inventory(runs: &[WorkflowRun], source: &str) -> Result<(), OrchestratorError> {
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
            || run.head_sha != source
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
    source: &str,
    token: &str,
    identity: CiIdentity,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let route = format!(
        "repos/tailrocks/velnor-new/actions/runs/{}/attempts/{}/jobs",
        identity.id, identity.attempt
    );
    let jobs = pagination::get_complete(
        &pagination::Collection {
            route: &route,
            name: "jobs",
            max_items: 2000,
            error: "image_publication_ci_job_inventory",
            identity_error: "image_publication_ci_job_identity",
            id: |job: &CiJob| job.id,
        },
        token,
        deadline,
    )?;
    for job in &jobs {
        validate_ci_job_identity(job, identity, source)?;
    }
    CiJobInventory { jobs }.validate_required_job()
}

struct CiJobInventory {
    jobs: Vec<CiJob>,
}

impl CiJobInventory {
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

fn validate_ci_job_identity(
    job: &CiJob,
    identity: CiIdentity,
    source_sha: &str,
) -> Result<(), OrchestratorError> {
    if job.run_id != Some(identity.id)
        || job.name.as_deref().is_none_or(str::is_empty)
        || job.head_sha.as_deref() != Some(source_sha)
        || job.status.as_deref().is_none_or(str::is_empty)
    {
        return Err(internal("image_publication_ci_job_identity"));
    }
    Ok(())
}
