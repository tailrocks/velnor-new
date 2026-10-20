//! Original manual tag authority for a cryptographically admitted generation.

use super::super::github::{GitHubApi, WorkflowArtifact};
use super::{
    ArtifactRef, artifacts, authenticate, authorization_for_context, fixed_path, observe_tag,
};
use crate::{
    OrchestratorError, internal::internal, package_release_signer::VerifiedAttestedSignedPackage,
};
use velnor_actions_contract::preview_production::ReleaseProductionContext;

pub(in crate::package_release_publish) fn verify_signed_origin(
    signed: &VerifiedAttestedSignedPackage,
) -> Result<(), OrchestratorError> {
    let plan = signed.plan();
    let receipt = signed.receipt();
    let context = ReleaseProductionContext {
        repository_id: receipt.repository.id,
        head_repository_id: receipt.repository.id,
        source_sha: receipt.source.commit_sha.clone(),
        source_ref: format!("refs/heads/{}", plan.default_branch),
        workflow_authority_sha: receipt.workflow.authority_sha.clone(),
        run_id: receipt.workflow.run_id,
        run_attempt: receipt.workflow.run_attempt,
        ci_run_id: receipt.source.ci_run_id,
        ci_run_attempt: receipt.source.ci_run_attempt,
        commit_count: 0,
        version: receipt.version.value.clone(),
    };
    let expected = authorization_for_context(plan, context)?;
    let api = GitHubApi::new(plan.repository.clone());
    authenticate(&api, plan, &expected.context, false)?;
    let jobs = api.complete_workflow_jobs(expected.context.run_id, expected.context.run_attempt)?;
    let tag_jobs: Vec<_> = jobs
        .jobs
        .iter()
        .filter(|job| job.name == "Authorize the permanent stable tag")
        .collect();
    if tag_jobs.len() != 1
        || tag_jobs[0].head_sha.as_deref() != Some(receipt.source.commit_sha.as_str())
        || tag_jobs[0].run_id != receipt.workflow.run_id
        || tag_jobs[0].status.as_deref() != Some("completed")
        || tag_jobs[0].conclusion.as_deref() != Some("success")
    {
        return Err(internal("stable_original_tag_job_unsuccessful"));
    }
    let directory = fixed_path("admission.json")?
        .parent()
        .ok_or_else(|| internal("stable_tag_temp"))?
        .to_owned();
    super::ensure_directory(&directory)?;
    let artifacts_list = api.workflow_artifacts(receipt.workflow.run_id)?;
    let origin_ref = locate(
        &artifacts_list,
        "origin",
        receipt.workflow.run_id,
        receipt.workflow.run_attempt,
    )?;
    let origin = artifacts::origin(&api, &expected, &origin_ref)?;
    if origin.tag != observe_tag(&api, plan)? || origin.tag.target_sha != receipt.source.commit_sha
    {
        return Err(internal("stable_original_tag_changed"));
    }
    let original = artifacts::intent(&api, &origin.authorization, &origin.intent)?;
    artifacts::require_intent_object(&origin, &original)?;
    Ok(())
}

fn locate(
    artifacts: &[WorkflowArtifact],
    kind: &str,
    run_id: u64,
    attempt: u32,
) -> Result<ArtifactRef, OrchestratorError> {
    let name = format!("velnor-stable-tag-{kind}-r{run_id}-a{attempt}");
    let matches: Vec<_> = artifacts
        .iter()
        .filter(|artifact| artifact.name == name)
        .collect();
    if matches.len() != 1 {
        return Err(internal("stable_original_artifact_ambiguous"));
    }
    let sha256 = matches[0]
        .digest
        .as_deref()
        .and_then(|value| value.strip_prefix("sha256:"))
        .filter(|value| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
        })
        .ok_or_else(|| internal("stable_original_artifact_digest"))?
        .to_owned();
    Ok(ArtifactRef {
        id: matches[0].id,
        sha256,
    })
}
