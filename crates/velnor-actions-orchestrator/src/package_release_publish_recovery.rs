//! Retained-run recovery admission and identity verification.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use velnor_actions_contract::{
    PackageReleasePlan,
    package_release::{PackageReleaseChannel, PackageReleaseEvent, PackageReleaseReceipt},
    parse_package_release_plan,
};

use crate::{
    OrchestratorError,
    internal::internal,
    package_release_signer::{
        PackageReleaseSigningIdentity, VerifiedAttestedSignedPackage,
        verify_attested_signed_package,
    },
};

use super::{
    GitHubPackagePublisher, OriginalVerification, ci_run_is_success, current_ci_from_env,
    github::{GitHubApi, WorkflowArtifact, WorkflowJobList, WorkflowRunResponse, sha256},
    identity::{is_git_sha, read_regular},
    publish_package_release,
};

const PLAN_FILE: &str = "package-release-plan.json";
const INPUT_DIR: &str = "preview-recovery-input";
const MAX_PLAN_BYTES: u64 = 1024 * 1024;
const MAX_ARCHIVE_BYTES: u64 = 1024 * 1024 * 1024;

pub(super) fn recover() -> Result<super::PackagePublicationOutcome, OrchestratorError> {
    let (signed, mode) = verified_original()?;
    let current_ci = current_ci_from_env()?;
    let mut backend = GitHubPackagePublisher::new(signed, current_ci, mode)?;
    publish_package_release(&mut backend)
}

/// Authenticate original bytes for promotion without release-write authority.
pub(crate) fn verify_original_for_promotion()
-> Result<VerifiedAttestedSignedPackage, OrchestratorError> {
    verified_original().map(|(signed, _)| signed)
}

fn verified_original()
-> Result<(VerifiedAttestedSignedPackage, OriginalVerification), OrchestratorError> {
    let root = runner_root()?;
    let plan_path = root.join(PLAN_FILE);
    if env_path("VELNOR_REQUEST_FILE")? != plan_path {
        return Err(internal("package_recovery_request_path_mismatch"));
    }
    let plan_bytes = read_regular_bounded(&plan_path, MAX_PLAN_BYTES)?;
    let plan = parse_package_release_plan(&plan_bytes)
        .map_err(|_| internal("package_recovery_plan_invalid"))?;
    validate_recovery_context(&plan)?;
    let original_run_id = positive_env("VELNOR_RELEASE_ORIGINAL_RUN_ID")?;
    let original_attempt = positive_env_u32("VELNOR_RELEASE_ORIGINAL_RUN_ATTEMPT")?;
    let repository_id = positive_env("GITHUB_REPOSITORY_ID")?;
    let api = GitHubApi::new(plan.repository.clone());
    api.verify_repository(repository_id)?;

    let artifact_name = format!("velnor-preview-signed-{original_run_id}-{original_attempt}");
    let artifacts = api.workflow_artifacts(original_run_id)?;
    let matches: Vec<_> = artifacts
        .iter()
        .filter(|artifact| artifact.name == artifact_name)
        .collect();
    if matches.len() != 1 {
        return Err(internal("package_recovery_artifact_not_unique"));
    }
    let artifact = matches[0];
    let artifact_source_sha = validate_artifact(
        artifact,
        original_run_id,
        repository_id,
        &plan.default_branch,
    )?;
    let zip_bytes = api.download_artifact_archive(artifact.id)?;
    if zip_bytes.len() as u64 != artifact.size_in_bytes
        || artifact
            .digest
            .as_deref()
            .is_some_and(|digest| digest != format!("sha256:{}", sha256(&zip_bytes)))
    {
        return Err(internal("package_recovery_artifact_digest_mismatch"));
    }

    let input = root.join(INPUT_DIR);
    create_fresh_directory(&input)?;
    let receipt = archive::extract_signed_assets(&zip_bytes, &input, &plan)?;
    if !receipt_matches_original(
        receipt.workflow.run_id,
        receipt.workflow.run_attempt,
        &receipt.source.commit_sha,
        receipt.version.commit_count,
        original_run_id,
        original_attempt,
        &artifact_source_sha,
    ) {
        return Err(internal("package_recovery_receipt_run_mismatch"));
    }
    let identity = original_signing_identity(
        &api,
        &plan,
        &receipt,
        original_run_id,
        original_attempt,
        repository_id,
    )?;
    let signed = verify_attested_signed_package(&input, &plan, &identity)?;
    let mode = OriginalVerification::RetainedRun {
        directory: Box::new(input),
        plan: Box::new(plan),
        identity: Box::new(identity),
    };
    Ok((signed, mode))
}

fn validate_recovery_context(plan: &PackageReleasePlan) -> Result<(), OrchestratorError> {
    if plan.channel != PackageReleaseChannel::Preview
        || env_text("GITHUB_EVENT_NAME")? != "workflow_dispatch"
        || env_text("GITHUB_REPOSITORY")? != plan.repository
        || env_text("GITHUB_REF")? != format!("refs/heads/{}", plan.default_branch)
        || env_text("GITHUB_WORKFLOW_REF")?
            != format!(
                "{}/{}@refs/heads/{}",
                plan.repository,
                plan.channel.workflow_path(),
                plan.default_branch
            )
        || !is_git_sha(&env_text("GITHUB_SHA")?)
        || env_text("GITHUB_WORKFLOW_SHA")? != env_text("GITHUB_SHA")?
    {
        return Err(internal("package_recovery_context_mismatch"));
    }
    Ok(())
}

fn validate_artifact(
    artifact: &WorkflowArtifact,
    run_id: u64,
    repository_id: u64,
    default_branch: &str,
) -> Result<String, OrchestratorError> {
    let Some(run) = artifact.workflow_run.as_ref() else {
        return Err(internal("package_recovery_artifact_run_missing"));
    };
    if artifact.expired
        || artifact.size_in_bytes == 0
        || artifact.size_in_bytes > MAX_ARCHIVE_BYTES
        || run.id != run_id
        || run.repository_id != repository_id
        || !is_git_sha(&run.head_sha)
        || run.head_branch != default_branch
    {
        return Err(internal("package_recovery_artifact_invalid"));
    }
    Ok(run.head_sha.clone())
}

fn receipt_matches_original(
    receipt_run_id: u64,
    receipt_attempt: u32,
    receipt_source_sha: &str,
    receipt_commit_count: Option<u64>,
    original_run_id: u64,
    original_attempt: u32,
    artifact_source_sha: &str,
) -> bool {
    receipt_run_id == original_run_id
        && receipt_attempt == original_attempt
        && receipt_source_sha == artifact_source_sha
        && receipt_commit_count.is_some_and(|count| count > 0)
}

fn original_signing_identity(
    api: &GitHubApi,
    plan: &PackageReleasePlan,
    receipt: &PackageReleaseReceipt,
    run_id: u64,
    attempt: u32,
    repository_id: u64,
) -> Result<PackageReleaseSigningIdentity, OrchestratorError> {
    let source = &receipt.source;
    let workflow_run = api.workflow_run(run_id, attempt)?;
    if !source_run_matches(
        &workflow_run,
        plan,
        source.commit_sha.as_str(),
        run_id,
        attempt,
        repository_id,
    ) || workflow_run.status.as_deref() != Some("completed")
    {
        return Err(internal("package_recovery_original_run_mismatch"));
    }
    let jobs = api.workflow_jobs(run_id, attempt)?;
    require_successful_job(
        &jobs,
        run_id,
        &source.commit_sha,
        plan.channel.signer_job_id(),
    )?;
    let source_ci = api.workflow_run(source.ci_run_id, source.ci_run_attempt)?;
    if !ci_run_is_success(
        &source_ci,
        source.ci_run_id,
        source.ci_run_attempt,
        repository_id,
        &source.commit_sha,
        &plan.default_branch,
    ) {
        return Err(internal("package_recovery_source_ci_invalid"));
    }
    let source_ref = format!("refs/heads/{}", plan.default_branch);
    let identity = PackageReleaseSigningIdentity {
        repository: plan.repository.clone(),
        repository_id,
        source_sha: source.commit_sha.clone(),
        source_ref: source_ref.clone(),
        default_branch: plan.default_branch.clone(),
        event: PackageReleaseEvent::Push,
        workflow_path: plan.channel.workflow_path().to_owned(),
        workflow_ref: format!(
            "{}/{}@{}",
            plan.repository,
            plan.channel.workflow_path(),
            source_ref
        ),
        workflow_sha: workflow_run.head_sha.unwrap_or_default(),
        run_id,
        run_attempt: attempt,
        ci_run_id: source.ci_run_id,
        ci_run_attempt: source.ci_run_attempt,
        commit_count: receipt.version.commit_count,
    };
    if identity.workflow_sha != source.commit_sha {
        return Err(internal("package_recovery_workflow_sha_mismatch"));
    }
    Ok(identity)
}

fn source_run_matches(
    run: &WorkflowRunResponse,
    plan: &PackageReleasePlan,
    source_sha: &str,
    run_id: u64,
    attempt: u32,
    repository_id: u64,
) -> bool {
    run.id == Some(run_id)
        && run.run_attempt == Some(attempt)
        && run.path.as_deref() == Some(plan.channel.workflow_path())
        && run.head_sha.as_deref() == Some(source_sha)
        && run.head_branch.as_deref() == Some(plan.default_branch.as_str())
        && run.event.as_deref() == Some("push")
        && run
            .repository
            .as_ref()
            .is_some_and(|repository| repository.id == repository_id)
}

fn require_successful_job(
    jobs: &WorkflowJobList,
    run_id: u64,
    source_sha: &str,
    required_name: &str,
) -> Result<(), OrchestratorError> {
    let matching: Vec<_> = jobs
        .jobs
        .iter()
        .filter(|job| job.name == required_name)
        .collect();
    if matching.len() != 1 {
        return Err(internal("package_recovery_signer_job_not_unique"));
    }
    let job = matching[0];
    if job.run_id != run_id
        || job.head_sha.as_deref() != Some(source_sha)
        || job.status.as_deref() != Some("completed")
        || job.conclusion.as_deref() != Some("success")
    {
        return Err(internal("package_recovery_signer_job_failed"));
    }
    Ok(())
}

#[path = "package_release_publish_recovery_archive.rs"]
mod archive;

fn create_fresh_directory(path: &Path) -> Result<(), OrchestratorError> {
    fs::create_dir(path).map_err(|_| internal("package_recovery_input_not_fresh"))
}

fn runner_root() -> Result<PathBuf, OrchestratorError> {
    let runner = PathBuf::from(env_text("RUNNER_TEMP")?);
    if !runner.is_absolute() {
        return Err(internal("package_recovery_runner_temp_invalid"));
    }
    let root = runner.join("velnor");
    let metadata =
        fs::symlink_metadata(&root).map_err(|_| internal("package_recovery_root_missing"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(internal("package_recovery_root_invalid"));
    }
    Ok(root)
}

fn env_path(key: &str) -> Result<PathBuf, OrchestratorError> {
    Ok(PathBuf::from(env_text(key)?))
}

fn env_text(key: &str) -> Result<String, OrchestratorError> {
    env::var(key)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal("package_recovery_context_missing"))
}

fn positive_env(key: &str) -> Result<u64, OrchestratorError> {
    env_text(key)?
        .parse::<u64>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| internal("package_recovery_numeric_context_invalid"))
}

fn positive_env_u32(key: &str) -> Result<u32, OrchestratorError> {
    env_text(key)?
        .parse::<u32>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| internal("package_recovery_numeric_context_invalid"))
}

fn read_regular_bounded(path: &Path, max: u64) -> Result<Vec<u8>, OrchestratorError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| internal("package_recovery_plan_missing"))?;
    if !metadata.file_type().is_file() || metadata.len() == 0 || metadata.len() > max {
        return Err(internal("package_recovery_plan_invalid"));
    }
    read_regular(path).map_err(|_| internal("package_recovery_plan_read"))
}

#[cfg(test)]
#[path = "package_release_publish_recovery_tests.rs"]
mod tests;
