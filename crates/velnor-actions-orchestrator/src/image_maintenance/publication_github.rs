// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeMap, env};

use base64::{Engine, engine::general_purpose::STANDARD};
use velnor_actions_mise::CheckDeadline;

use crate::{OrchestratorError, internal::internal};

#[path = "publication_github/read.rs"]
mod read;
use self::read::get_json;

#[path = "publication_github_models.rs"]
mod models;
use models::{
    ArtifactList, Branch, Comparison, ContentFile, JobList, Repository, Run, WorkflowRuns,
};

use super::{ImagePublicationProfile, PublishImageRequest, authority};

const CALLER_WORKFLOW: &str = ".github/workflows/product-release.yml";
const RUNNER_WORKFLOW: &str = ".github/workflows/product-release-images.yml";
const JOB_WORKFLOW: &str = ".github/workflows/job-image-candidate.yml";
const CI_WORKFLOW: &str = ".github/workflows/ci.yml";

#[derive(Debug, Clone)]
pub(super) struct AuthenticatedProducer {
    pub head_sha: String,
    pub workflow_path: String,
    pub ci_run_id: u64,
    pub ci_run_attempt: u32,
}

pub(super) fn authenticate(
    request: &PublishImageRequest,
    candidate: &authority::CandidateRecord,
    deadline: CheckDeadline,
) -> Result<(AuthenticatedProducer, BTreeMap<String, Vec<u8>>), OrchestratorError> {
    let token = env::var("GH_TOKEN").map_err(|_| internal("image_publication_token_missing"))?;
    if token.is_empty() {
        return Err(internal("image_publication_token_missing"));
    }
    let repository: Repository = get_json("repos/tailrocks/velnor-new", &token, deadline)?;
    if repository.id != authority::IMAGE_REPOSITORY_ID
        || repository.full_name != authority::IMAGE_REPOSITORY
        || repository.default_branch != "main"
    {
        return Err(internal("image_publication_repository_authority"));
    }
    let branch: Branch = get_json("repos/tailrocks/velnor-new/branches/main", &token, deadline)?;
    if branch.name != "main" || !is_sha1(&branch.commit.sha) {
        return Err(internal("image_publication_default_branch"));
    }
    verify_source_on_main(request, &token, &branch.commit.sha, deadline)?;
    let run = load_producer_run(request, &token, deadline)?;
    validate_producer_run(request, candidate, &run)?;
    validate_artifact(request, candidate, &run, &token, deadline)?;
    validate_producer_jobs(request, &token, deadline)?;
    let (ci_run_id, ci_run_attempt) = latest_successful_ci(request, &token, deadline)?;
    let source_files = source_files(request, candidate, &token, deadline)?;
    authority::verify_source_inputs(candidate, &source_files)?;
    let (signer_workflow_path, signer_workflow_sha) = match request.profile {
        ImagePublicationProfile::Job => (run.path.as_str(), run.head_sha.as_str()),
        ImagePublicationProfile::Runner
        | ImagePublicationProfile::Dind
        | ImagePublicationProfile::ResourceProbe => (
            RUNNER_WORKFLOW,
            candidate
                .workflow
                .referenced_workflow_sha
                .as_deref()
                .ok_or_else(|| internal("image_publication_referenced_workflow"))?,
        ),
    };
    super::attestation::verify(
        request,
        candidate,
        &super::attestation::AttestationRun {
            caller_workflow_path: &run.path,
            caller_workflow_sha: &run.head_sha,
            signer_workflow_path,
            signer_workflow_sha,
            source_sha: &run.head_sha,
            event: &run.event,
            run_id: run.id,
            run_attempt: run.attempt,
        },
        deadline,
    )?;
    if authority::record_file_digest(&request.record_path)? != candidate.record_sha256 {
        return Err(internal("image_publication_record_changed"));
    }
    authority::verify_archive_bytes(request, candidate)?;
    Ok((
        AuthenticatedProducer {
            head_sha: run.head_sha,
            workflow_path: run.path,
            ci_run_id,
            ci_run_attempt,
        },
        source_files,
    ))
}

fn verify_source_on_main(
    request: &PublishImageRequest,
    token: &str,
    main_sha: &str,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    if request.profile == ImagePublicationProfile::Job && request.source_commit != main_sha {
        return Err(internal("image_publication_job_source_not_main"));
    }
    let route = format!(
        "repos/tailrocks/velnor-new/compare/{}...{}",
        request.source_commit, main_sha
    );
    let comparison: Comparison = get_json(&route, token, deadline)?;
    if !matches!(comparison.status.as_str(), "ahead" | "identical") {
        return Err(internal("image_publication_source_not_on_main"));
    }
    Ok(())
}

fn load_producer_run(
    request: &PublishImageRequest,
    token: &str,
    deadline: CheckDeadline,
) -> Result<Run, OrchestratorError> {
    if request.producer_run_id == 0 || request.producer_run_attempt == 0 {
        return Err(internal("image_publication_run_expectation"));
    }
    let route = format!(
        "repos/tailrocks/velnor-new/actions/runs/{}/attempts/{}",
        request.producer_run_id, request.producer_run_attempt
    );
    get_json(&route, token, deadline)
}

fn validate_producer_run(
    request: &PublishImageRequest,
    candidate: &authority::CandidateRecord,
    run: &Run,
) -> Result<(), OrchestratorError> {
    let path = match request.profile {
        ImagePublicationProfile::Job => JOB_WORKFLOW,
        ImagePublicationProfile::Runner
        | ImagePublicationProfile::Dind
        | ImagePublicationProfile::ResourceProbe => CALLER_WORKFLOW,
    };
    let repository = run.head_repository.as_ref();
    if run.id != request.producer_run_id
        || run.attempt != request.producer_run_attempt
        || run.event != "workflow_dispatch"
        || run.path != path
        || run.head_branch != "main"
        || repository.is_none_or(|repo| {
            repo.id != authority::IMAGE_REPOSITORY_ID
                || repo.full_name != authority::IMAGE_REPOSITORY
        })
        || !is_sha1(&run.head_sha)
        || candidate.workflow.event != run.event
        || candidate.workflow.workflow_ref
            != format!("{}/{}@refs/heads/main", authority::IMAGE_REPOSITORY, path)
        || candidate.workflow.authority_commit != run.head_sha
    {
        return Err(internal("image_publication_producer_run_identity"));
    }
    if (request.profile == ImagePublicationProfile::Job && run.head_sha != request.source_commit)
        || (run.status == "completed" && run.conclusion.as_deref() != Some("success"))
    {
        return Err(internal("image_publication_job_run_identity"));
    }
    if request.profile != ImagePublicationProfile::Job {
        validate_referenced_workflow(candidate, run)?;
    }
    Ok(())
}

fn validate_referenced_workflow(
    candidate: &authority::CandidateRecord,
    run: &Run,
) -> Result<(), OrchestratorError> {
    let matching = run
        .referenced_workflows
        .iter()
        .filter(|workflow| {
            workflow.path
                == format!(
                    "{}/{}@{}",
                    authority::IMAGE_REPOSITORY,
                    RUNNER_WORKFLOW,
                    workflow.sha
                )
                && workflow.reference == "refs/heads/main"
        })
        .collect::<Vec<_>>();
    if matching.len() != 1
        || candidate.workflow.referenced_workflow_path.as_deref()
            != matching.first().map(|workflow| workflow.path.as_str())
        || candidate.workflow.referenced_workflow_ref.as_deref()
            != matching.first().map(|workflow| workflow.reference.as_str())
        || candidate.workflow.referenced_workflow_sha.as_deref()
            != matching.first().map(|workflow| workflow.sha.as_str())
        || matching
            .first()
            .is_none_or(|workflow| !is_sha1(&workflow.sha))
        || matching
            .first()
            .is_none_or(|workflow| workflow.sha != run.head_sha)
    {
        return Err(internal("image_publication_referenced_workflow"));
    }
    Ok(())
}

fn validate_artifact(
    request: &PublishImageRequest,
    candidate: &authority::CandidateRecord,
    run: &Run,
    token: &str,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    if request.artifact_id == 0 {
        return Err(internal("image_publication_artifact_expectation"));
    }
    let route = format!(
        "repos/tailrocks/velnor-new/actions/runs/{}/artifacts?per_page=100",
        request.producer_run_id
    );
    let response: ArtifactList = get_json(&route, token, deadline)?;
    let expected = artifact_name(request.profile, request.producer_run_attempt);
    let matches = response
        .artifacts
        .iter()
        .filter(|artifact| artifact.name == expected)
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(internal("image_publication_artifact_count"));
    }
    let artifact = matches[0];
    if artifact.id != request.artifact_id
        || artifact.expired
        || artifact.size_in_bytes == 0
        || artifact.workflow_run.id != run.id
        || artifact.workflow_run.head_sha != run.head_sha
        || artifact.workflow_run.head_branch != "main"
        || (request.profile == ImagePublicationProfile::Job
            && (artifact.workflow_run.head_sha != candidate.source_commit
                || artifact.name != format!("velnor-job-image-candidate-{}", run.attempt)))
    {
        return Err(internal("image_publication_artifact_identity"));
    }
    Ok(())
}

fn artifact_name(profile: ImagePublicationProfile, attempt: u32) -> String {
    match profile {
        ImagePublicationProfile::Job => format!("velnor-job-image-candidate-{attempt}"),
        ImagePublicationProfile::Runner
        | ImagePublicationProfile::Dind
        | ImagePublicationProfile::ResourceProbe => format!("image-assets-{attempt}"),
    }
}

fn validate_producer_jobs(
    request: &PublishImageRequest,
    token: &str,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let route = format!(
        "repos/tailrocks/velnor-new/actions/runs/{}/attempts/{}/jobs?per_page=100",
        request.producer_run_id, request.producer_run_attempt
    );
    let response: JobList = get_json(&route, token, deadline)?;
    let expected = producer_job_names(request.profile);
    for name in expected {
        let matches = response
            .jobs
            .iter()
            .filter(|job| job.name == *name)
            .collect::<Vec<_>>();
        if matches.len() != 1
            || matches[0].status != "completed"
            || matches[0].conclusion.as_deref() != Some("success")
        {
            return Err(internal("image_publication_producer_job"));
        }
    }
    Ok(())
}

fn producer_job_names(profile: ImagePublicationProfile) -> &'static [&'static str] {
    match profile {
        ImagePublicationProfile::Job => &[
            "Build Linux job image candidate",
            "Attest Linux job image candidate",
        ],
        ImagePublicationProfile::Runner
        | ImagePublicationProfile::Dind
        | ImagePublicationProfile::ResourceProbe => &[
            "Build runner images",
            "Attest runner images",
            "qualify-images",
        ],
    }
}

fn latest_successful_ci(
    request: &PublishImageRequest,
    token: &str,
    deadline: CheckDeadline,
) -> Result<(u64, u32), OrchestratorError> {
    let route = format!(
        "repos/tailrocks/velnor-new/actions/workflows/ci.yml/runs?branch=main&event=push&head_sha={}&per_page=100",
        request.source_commit
    );
    let response: WorkflowRuns = get_json(&route, token, deadline)?;
    let mut runs = response
        .workflow_runs
        .into_iter()
        .filter(|run| {
            run.path == CI_WORKFLOW
                && run.event == "push"
                && run.head_branch == "main"
                && run.head_sha == request.source_commit
                && run.head_repository.as_ref().is_some_and(|repo| {
                    repo.id == authority::IMAGE_REPOSITORY_ID
                        && repo.full_name == authority::IMAGE_REPOSITORY
                })
        })
        .collect::<Vec<_>>();
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
        || latest.run_attempt == 0
    {
        return Err(internal("image_publication_ci_latest_not_success"));
    }
    Ok((latest.id, latest.run_attempt))
}

fn source_files(
    request: &PublishImageRequest,
    candidate: &authority::CandidateRecord,
    token: &str,
    deadline: CheckDeadline,
) -> Result<BTreeMap<String, Vec<u8>>, OrchestratorError> {
    authority::source_paths(request.profile)
        .iter()
        .map(|path| {
            let route = format!(
                "repos/tailrocks/velnor-new/contents/{path}?ref={}",
                request.source_commit
            );
            let content: ContentFile = get_json(&route, token, deadline)?;
            if content.encoding != "base64" {
                return Err(internal("image_publication_source_encoding"));
            }
            let compact = content
                .content
                .bytes()
                .filter(|byte| !byte.is_ascii_whitespace())
                .collect::<Vec<_>>();
            let bytes = STANDARD
                .decode(compact)
                .map_err(|_| internal("image_publication_source_base64"))?;
            if !candidate.source_inputs.contains_key(*path) {
                return Err(internal("image_publication_source_path"));
            }
            Ok(((*path).to_owned(), bytes))
        })
        .collect()
}

fn is_sha1(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
