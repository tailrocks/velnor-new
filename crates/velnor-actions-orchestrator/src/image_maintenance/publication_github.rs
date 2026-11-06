// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeMap, env};

use base64::{Engine, engine::general_purpose::STANDARD};
use velnor_actions_mise::CheckDeadline;

use crate::{OrchestratorError, internal::internal};

#[path = "publication_github/read.rs"]
pub(in crate::image_maintenance) mod read;
use self::read::get_json;

#[path = "publication_github/attestor.rs"]
mod attestor;
#[path = "publication_github/ci.rs"]
pub(in crate::image_maintenance) mod ci;
#[path = "publication_github/pagination.rs"]
mod pagination;
#[path = "publication_github/publisher.rs"]
mod publisher;
#[path = "publication_github/snapshot.rs"]
mod snapshot;

#[path = "publication_github_models.rs"]
mod models;
use models::{Artifact, Comparison, ContentFile, Job, Run};

use super::{ImagePublicationProfile, PublishImageRequest, authority};

const CI_WORKFLOW: &str = ".github/workflows/ci.yml";

#[derive(Debug, Clone)]
pub(super) struct AuthenticatedProducer {
    pub head_sha: String,
    pub workflow_path: String,
    pub main_sha: String,
    pub ci_run_id: u64,
    pub ci_run_attempt: u32,
}

pub(super) fn authenticate(
    request: &PublishImageRequest,
    candidate: &authority::CandidateRecord,
    proof: Option<&super::qualification::AdmittedProof>,
    deadline: CheckDeadline,
) -> Result<(AuthenticatedProducer, BTreeMap<String, Vec<u8>>), OrchestratorError> {
    let token = github_token()?;
    publisher::authenticate(request, &token, deadline)?;
    let main_sha = ci::main_sha(&token, deadline)?;
    verify_source_on_main(request, &token, &main_sha, deadline)?;
    let run = load_producer_run(request, &token, deadline)?;
    validate_producer_run(request, candidate, &run)?;
    validate_artifact(request, candidate, &run, &token, deadline)?;
    let ci = ci::authenticate(request, &token, deadline)?;
    if ci.id != run.id || ci.attempt != run.attempt {
        return Err(internal("image_publication_producer_ci_mismatch"));
    }
    let source_files = source_files(request, &token, deadline)?;
    authority::verify_source_inputs(candidate, &source_files)?;
    if let Some(proof) = proof {
        let qualifier = attestor::runner(request, proof, &token, deadline)?;
        let identity = crate::package_release_signer::ImageAttestationIdentity {
            repository: authority::IMAGE_REPOSITORY,
            repository_id: authority::IMAGE_REPOSITORY_ID,
            caller_workflow_path: &qualifier.path,
            caller_workflow_sha: &qualifier.head_sha,
            signer_workflow_path: super::qualification::SIGNER_WORKFLOW,
            signer_workflow_sha: &qualifier.head_sha,
            signer_ref: "refs/heads/main",
            source_sha: &qualifier.head_sha,
            source_ref: "refs/heads/main",
            event: &qualifier.event,
            run_id: qualifier.id,
            run_attempt: qualifier.attempt,
        };
        crate::package_release_signer::verify_image_file_attestation(
            &proof.path,
            &identity,
            &super::qualification::expected_subject(proof),
            &[],
            deadline,
        )?;
        super::qualification::unchanged(proof)?;
    } else {
        let job_run = attestor::job(request, &token, deadline)?;
        super::attestation::verify(
            request,
            candidate,
            &super::attestation::AttestationRun {
                caller_workflow_path: &job_run.path,
                caller_workflow_sha: &job_run.head_sha,
                signer_workflow_path: &job_run.path,
                signer_workflow_sha: &job_run.head_sha,
                source_sha: &job_run.head_sha,
                event: &job_run.event,
                run_id: job_run.id,
                run_attempt: job_run.attempt,
            },
            &source_files,
            deadline,
        )?;
    }
    if authority::record_file_digest(&request.record_path)? != candidate.record_sha256 {
        return Err(internal("image_publication_record_changed"));
    }
    authority::verify_archive_bytes(request, candidate)?;
    Ok((
        AuthenticatedProducer {
            head_sha: run.head_sha,
            workflow_path: run.path,
            main_sha,
            ci_run_id: ci.id,
            ci_run_attempt: ci.attempt,
        },
        source_files,
    ))
}

pub(super) fn revalidate_before_publish(
    request: &PublishImageRequest,
    producer: &AuthenticatedProducer,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let token = github_token()?;
    publisher::authenticate(request, &token, deadline)?;
    ci::revalidate(request, producer, &token, deadline)
}

fn github_token() -> Result<String, OrchestratorError> {
    let token = env::var("GH_TOKEN").map_err(|_| internal("image_publication_token_missing"))?;
    if token.is_empty() {
        return Err(internal("image_publication_token_missing"));
    }
    Ok(token)
}

pub(super) fn verify_source_on_main(
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
    let path = CI_WORKFLOW;
    let repository = run.head_repository.as_ref();
    if run.id != request.producer_run_id
        || run.attempt != request.producer_run_attempt
        || run.event != "push"
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
    if request.profile == ImagePublicationProfile::Job && run.head_sha != request.source_commit {
        return Err(internal("image_publication_job_run_identity"));
    }
    if run.head_sha != request.source_commit {
        return Err(internal("image_publication_producer_source_mismatch"));
    }
    if run.status != "completed" || run.conclusion.as_deref() != Some("success") {
        return Err(internal("image_publication_job_run_identity"));
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
        "repos/tailrocks/velnor-new/actions/runs/{}/artifacts",
        request.producer_run_id
    );
    let artifacts = pagination::get_complete(
        &pagination::Collection {
            route: &route,
            name: "artifacts",
            max_items: 2000,
            error: "image_publication_artifact_inventory",
            identity_error: "image_publication_artifact_identity",
            id: |artifact: &Artifact| Some(artifact.id),
        },
        token,
        deadline,
    )?;
    let expected = artifact_name(request.profile, request.producer_run_attempt);
    let matches = artifacts
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

fn source_files(
    request: &PublishImageRequest,
    token: &str,
    deadline: CheckDeadline,
) -> Result<BTreeMap<String, Vec<u8>>, OrchestratorError> {
    if request.profile == ImagePublicationProfile::Job {
        return snapshot::source_files(
            &request.source_commit,
            authority::source_paths(request.profile),
            token,
            deadline,
        );
    }
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
