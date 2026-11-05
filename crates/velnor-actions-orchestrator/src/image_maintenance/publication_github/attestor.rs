// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::{PublishImageRequest, authority, qualification};
use super::{Job, Run, get_json, pagination};
use crate::{OrchestratorError, internal::internal};
use velnor_actions_mise::CheckDeadline;

const RUNNER_CALLER: &str = ".github/workflows/runner-image-qualification.yml";

pub(super) fn runner(
    request: &PublishImageRequest,
    proof: &qualification::AdmittedProof,
    token: &str,
    deadline: CheckDeadline,
) -> Result<Run, OrchestratorError> {
    let run = load(
        request,
        proof.qualifier_run_id,
        proof.qualifier_run_attempt,
        RUNNER_CALLER,
        token,
        deadline,
    )?;
    let expected_path = format!(
        "{}/{}@{}",
        authority::IMAGE_REPOSITORY,
        qualification::SIGNER_WORKFLOW,
        run.head_sha
    );
    let referenced = run
        .referenced_workflows
        .iter()
        .filter(|workflow| {
            workflow.path == expected_path
                && matches!(
                    workflow.reference.as_deref(),
                    None | Some("refs/heads/main")
                )
                && workflow.sha == run.head_sha
        })
        .count();
    if referenced != 1 {
        return Err(internal("image_qualification_signer_workflow"));
    }
    let route = format!(
        "repos/tailrocks/velnor-new/actions/runs/{}/attempts/{}/jobs",
        run.id, run.attempt
    );
    let jobs = pagination::get_complete(
        &pagination::Collection {
            route: &route,
            name: "jobs",
            max_items: 2000,
            error: "image_qualification_job_inventory",
            identity_error: "image_qualification_proof_job",
            id: |job: &Job| job.id,
        },
        token,
        deadline,
    )?;
    let matching = jobs
        .iter()
        .filter(|job| job.name == "qualify-images")
        .collect::<Vec<_>>();
    if matching.len() != 1
        || matching[0].id != Some(proof.qualifier_job_id)
        || matching[0].status != "completed"
        || matching[0].conclusion.as_deref() != Some("success")
    {
        return Err(internal("image_qualification_proof_job"));
    }
    Ok(run)
}

pub(super) fn job(
    request: &PublishImageRequest,
    token: &str,
    deadline: CheckDeadline,
) -> Result<Run, OrchestratorError> {
    let id = request
        .job_attestation_run_id
        .ok_or_else(|| internal("image_job_attestation_run_required"))?;
    let attempt = request
        .job_attestation_run_attempt
        .ok_or_else(|| internal("image_job_attestation_run_required"))?;
    let run = load(
        request,
        id,
        attempt,
        ".github/workflows/job-image-candidate.yml",
        token,
        deadline,
    )?;
    let route = format!(
        "repos/tailrocks/velnor-new/actions/runs/{}/attempts/{}/jobs",
        run.id, run.attempt
    );
    let jobs = pagination::get_complete(
        &pagination::Collection {
            route: &route,
            name: "jobs",
            max_items: 2000,
            error: "image_job_attestation_inventory",
            identity_error: "image_job_attestation_job",
            id: |job: &Job| job.id,
        },
        token,
        deadline,
    )?;
    let matching = jobs
        .iter()
        .filter(|job| job.name == "Attest Linux job image candidate")
        .collect::<Vec<_>>();
    if matching.len() != 1
        || matching[0].status != "completed"
        || matching[0].conclusion.as_deref() != Some("success")
    {
        return Err(internal("image_job_attestation_job"));
    }
    Ok(run)
}

fn load(
    request: &PublishImageRequest,
    id: u64,
    attempt: u32,
    path: &str,
    token: &str,
    deadline: CheckDeadline,
) -> Result<Run, OrchestratorError> {
    let current = std::env::var("GITHUB_RUN_ID")
        .ok()
        .and_then(|value| value.parse::<u64>().ok());
    if id == 0 || attempt == 0 || id == request.producer_run_id || Some(id) == current {
        return Err(internal("image_attestor_run_identity"));
    }
    let route = format!("repos/tailrocks/velnor-new/actions/runs/{id}/attempts/{attempt}");
    let run: Run = get_json(&route, token, deadline)?;
    if run.id != id
        || run.attempt != attempt
        || run.event != "push"
        || run.path != path
        || run.head_branch != "main"
        || run.head_sha != request.source_commit
        || run.status != "completed"
        || run.conclusion.as_deref() != Some("success")
        || run.head_repository.as_ref().is_none_or(|repo| {
            repo.id != authority::IMAGE_REPOSITORY_ID
                || repo.full_name != authority::IMAGE_REPOSITORY
        })
    {
        return Err(internal("image_attestor_run_identity"));
    }
    Ok(run)
}
