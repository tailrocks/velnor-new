//! Retained-run recovery admission and identity verification.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use velnor_actions_contract::{
    PackageReleasePlan, PackageReleasePolicy,
    package_release::{PackageReleaseChannel, PackageReleaseReceipt},
    parse_package_release_policy,
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
    GitHubPackagePublisher, OriginalVerification, current_ci_from_env,
    github::{
        GitHubApi, WorkflowArtifact, WorkflowJobList, WorkflowMetadata, WorkflowRunResponse, sha256,
    },
    identity::is_git_sha,
    publish_backend,
    run_identity::{RunSource, run_matches},
};

#[path = "package_release_promotion_context.rs"]
pub(super) mod promotion_context;
use promotion_context::{Invocation, Purpose};

const POLICY_FILE: &str = "package-release-policy.json";
const INPUT_DIR: &str = "release-recovery-input";
const SOURCE_SELECTION_INPUT_DIR: &str = "preview-recovery-selection-input";
const FINALIZATION_INPUT_DIR: &str = "preview-recovery-finalization-input";
const MAX_PLAN_BYTES: u64 = 1024 * 1024;
const MAX_ARCHIVE_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::package_release_publish) enum PreviewRecoveryVerificationStage {
    SourceSelection,
    Finalization,
}

impl PreviewRecoveryVerificationStage {
    fn input_directory(self, root: &Path) -> PathBuf {
        match self {
            Self::SourceSelection => root.join(SOURCE_SELECTION_INPUT_DIR),
            Self::Finalization => root.join(FINALIZATION_INPUT_DIR),
        }
    }
}

pub(super) fn recover() -> Result<super::PackagePublicationOutcome, OrchestratorError> {
    let (signed, mode) = verified_original(Purpose::PublicationRecovery)?;
    let current_ci = current_ci_from_env()?;
    let mut backend = GitHubPackagePublisher::new(signed, current_ci, mode)?;
    publish_backend(&mut backend)
}

/// Authenticate original bytes for promotion without release-write authority.
pub(crate) fn verify_original_for_promotion()
-> Result<VerifiedAttestedSignedPackage, OrchestratorError> {
    super::artifact_zip::within_operation(|| {
        let (signed, _) = verified_original(Purpose::TapPromotion)?;
        if Invocation::from_env()?.continuation(signed.plan()) {
            super::continuation::verify_published_selection(&signed)?;
        }
        Ok(signed)
    })
}

/// Authenticate the retained native preview receipt before selecting source A
/// for a manual recovery coordinator. The seed contributes repository custody
/// only; the policy and plan are read from the exact receipt source.
pub(in crate::package_release_publish) fn verify_preview_original_for_source_selection(
    seed: &PackageReleasePolicy,
    stage: PreviewRecoveryVerificationStage,
) -> Result<VerifiedAttestedSignedPackage, OrchestratorError> {
    if seed.channel != PackageReleaseChannel::Preview
        || seed.default_branch != "main"
        || !velnor_actions_contract::vcs::is_safe_github_repository(&seed.repository)
    {
        return Err(internal("package_recovery_source_seed_invalid"));
    }
    let root = runner_root()?;
    let original_run_id = positive_env("VELNOR_RELEASE_ORIGINAL_RUN_ID")?;
    let original_attempt = positive_env_u32("VELNOR_RELEASE_ORIGINAL_RUN_ATTEMPT")?;
    let repository_id = positive_env("GITHUB_REPOSITORY_ID")?;
    let api = GitHubApi::new(seed.repository.clone());
    api.verify_repository(repository_id)?;

    let (authority_sha, zip_bytes) =
        original_archive(&api, seed, original_run_id, original_attempt, repository_id)?;
    let source_sha = archive::preview_source_sha(&zip_bytes)?;
    let (source_policy, plan) = super::release_policy_admission::read_preview_policy_at_source(
        &api,
        &source_sha,
        &seed.repository,
        &seed.default_branch,
    )?;
    if source_policy.channel != PackageReleaseChannel::Preview
        || source_policy.repository != seed.repository
        || source_policy.default_branch != seed.default_branch
    {
        return Err(internal("package_recovery_source_policy_mismatch"));
    }

    let input = stage.input_directory(&root);
    create_fresh_directory(&input)?;
    let receipt = archive::extract_signed_assets(&zip_bytes, &input, &plan)?;
    if !receipt_matches_original(
        receipt.workflow.run_id,
        receipt.workflow.run_attempt,
        &receipt.source.commit_sha,
        (PackageReleaseChannel::Preview, receipt.version.commit_count),
        original_run_id,
        original_attempt,
        &source_sha,
    ) || receipt.workflow.authority_sha != authority_sha
    {
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

    // Preserve the original receipt's CI identity, but independently require
    // the latest exact-A CI attempt to be successful before admitting A.
    let ci_source = RunSource {
        repository_id,
        sha: &source_sha,
        branch: &seed.default_branch,
    };
    let ci_workflow = api.admitted_workflow(
        velnor_actions_contract::CI_WORKFLOW_PATH,
        velnor_actions_contract::WORKFLOW_DISPLAY_NAME,
    )?;
    let latest_ci = api.latest_source_ci(&ci_workflow, ci_source)?;
    let latest_id = latest_ci
        .id
        .filter(|id| *id > 0)
        .ok_or_else(|| internal("package_ci_latest_attempt_mismatch"))?;
    let latest_attempt = latest_ci
        .run_attempt
        .filter(|attempt| *attempt > 0)
        .ok_or_else(|| internal("package_ci_latest_attempt_mismatch"))?;
    api.verify_ci_identity(latest_id, latest_attempt, ci_source, true)?;

    verify_attested_signed_package(&input, &plan, &identity)
}

fn verified_original(
    purpose: Purpose,
) -> Result<(VerifiedAttestedSignedPackage, OriginalVerification), OrchestratorError> {
    let root = runner_root()?;
    let policy = recovery_policy(&root)?;
    let original_run_id = positive_env("VELNOR_RELEASE_ORIGINAL_RUN_ID")?;
    let original_attempt = positive_env_u32("VELNOR_RELEASE_ORIGINAL_RUN_ATTEMPT")?;
    let repository_id = positive_env("GITHUB_REPOSITORY_ID")?;
    let api = GitHubApi::new(policy.repository.clone());
    api.verify_repository(repository_id)?;

    let (artifact_workflow_sha, zip_bytes) = original_archive(
        &api,
        &policy,
        original_run_id,
        original_attempt,
        repository_id,
    )?;
    let artifact_source_sha = match policy.channel {
        PackageReleaseChannel::Preview => archive::preview_source_sha(&zip_bytes)?,
        PackageReleaseChannel::Stable => artifact_workflow_sha.clone(),
    };
    let plan =
        super::release_policy_admission::resolve_policy_at_source(&policy, &artifact_source_sha)?;
    validate_recovery_context(&plan, purpose)?;

    let input = root.join(INPUT_DIR);
    create_fresh_directory(&input)?;
    let receipt = archive::extract_signed_assets(&zip_bytes, &input, &plan)?;
    if !receipt_matches_original(
        receipt.workflow.run_id,
        receipt.workflow.run_attempt,
        &receipt.source.commit_sha,
        (plan.channel, receipt.version.commit_count),
        original_run_id,
        original_attempt,
        &artifact_source_sha,
    ) || (plan.channel == PackageReleaseChannel::Preview
        && receipt.workflow.authority_sha != artifact_workflow_sha)
    {
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

fn recovery_policy(root: &Path) -> Result<PackageReleasePolicy, OrchestratorError> {
    let plan_path = root.join(POLICY_FILE);
    if env_path("VELNOR_REQUEST_FILE")? != plan_path {
        return Err(internal("package_recovery_request_path_mismatch"));
    }
    let plan_bytes = read_regular_bounded(&plan_path, MAX_PLAN_BYTES)?;
    let policy = parse_package_release_policy(&plan_bytes)
        .map_err(|_| internal("package_recovery_policy_invalid"))?;
    if policy
        .canonical_bytes()
        .map_err(|_| internal("package_recovery_policy_encoding"))?
        != env_text("VELNOR_RELEASE_POLICY")?.as_bytes()
    {
        return Err(internal("package_recovery_policy_substitution"));
    }
    Ok(policy)
}

fn original_archive(
    api: &GitHubApi,
    policy: &PackageReleasePolicy,
    original_run_id: u64,
    original_attempt: u32,
    repository_id: u64,
) -> Result<(String, Vec<u8>), OrchestratorError> {
    let channel = match policy.channel {
        PackageReleaseChannel::Preview => "preview",
        PackageReleaseChannel::Stable => "stable",
    };
    let artifact_name = format!("velnor-{channel}-signed-{original_run_id}-{original_attempt}");
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
        &policy.default_branch,
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
    Ok((artifact_source_sha, zip_bytes))
}

fn validate_recovery_context(
    plan: &PackageReleasePlan,
    purpose: Purpose,
) -> Result<(), OrchestratorError> {
    Invocation::from_env()?.validate(plan, purpose)
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
    version: (PackageReleaseChannel, Option<u64>),
    original_run_id: u64,
    original_attempt: u32,
    artifact_source_sha: &str,
) -> bool {
    receipt_run_id == original_run_id
        && receipt_attempt == original_attempt
        && receipt_source_sha == artifact_source_sha
        && match version {
            (PackageReleaseChannel::Preview, Some(count)) => count > 0,
            (PackageReleaseChannel::Stable, None) => true,
            _ => false,
        }
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
    let workflow =
        api.admitted_workflow(plan.channel.workflow_path(), plan.channel.workflow_name())?;
    let workflow_run = api.workflow_run(run_id, attempt)?;
    if !source_run_matches(
        &workflow_run,
        plan,
        receipt.workflow.authority_sha.as_str(),
        run_id,
        attempt,
        repository_id,
        &workflow,
    ) || workflow_run.status.as_deref() != Some("completed")
    {
        return Err(internal("package_recovery_original_run_mismatch"));
    }
    let jobs = api.complete_workflow_jobs(run_id, attempt)?;
    require_successful_job(
        &jobs,
        run_id,
        &receipt.workflow.authority_sha,
        plan.channel.signer_job_id(),
    )?;
    if workflow_run.head_sha.as_deref() != Some(receipt.workflow.authority_sha.as_str()) {
        return Err(internal("package_recovery_workflow_sha_mismatch"));
    }
    if plan.channel == PackageReleaseChannel::Preview {
        api.verify_preview_main_ancestry(
            repository_id,
            &plan.default_branch,
            &source.commit_sha,
            &receipt.workflow.authority_sha,
        )?;
    }
    api.verify_ci_identity(
        source.ci_run_id,
        source.ci_run_attempt,
        RunSource {
            repository_id,
            sha: &source.commit_sha,
            branch: &plan.default_branch,
        },
        false,
    )?;
    let source_ref = format!("refs/heads/{}", plan.default_branch);
    let identity = PackageReleaseSigningIdentity {
        repository: plan.repository.clone(),
        repository_id,
        source_sha: source.commit_sha.clone(),
        source_ref: source_ref.clone(),
        default_branch: plan.default_branch.clone(),
        event: plan.channel.event(),
        workflow_path: plan.channel.workflow_path().to_owned(),
        workflow_ref: format!(
            "{}/{}@{}",
            plan.repository,
            plan.channel.workflow_path(),
            source_ref
        ),
        workflow_sha: receipt.workflow.authority_sha.clone(),
        run_id,
        run_attempt: attempt,
        ci_run_id: source.ci_run_id,
        ci_run_attempt: source.ci_run_attempt,
        commit_count: receipt.version.commit_count,
    };
    if plan.channel == PackageReleaseChannel::Stable && identity.workflow_sha != source.commit_sha {
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
    workflow: &WorkflowMetadata,
) -> bool {
    run_matches(
        run,
        workflow,
        run_id,
        attempt,
        RunSource {
            repository_id,
            sha: source_sha,
            branch: &plan.default_branch,
        },
        plan.channel.event().as_str(),
    )
}

pub(super) fn require_successful_job(
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
    super::release_policy_admission::metadata::read(path)
        .map_err(|_| internal("package_recovery_plan_read"))
}

#[cfg(test)]
#[path = "package_release_publish_recovery_tests.rs"]
mod tests;
