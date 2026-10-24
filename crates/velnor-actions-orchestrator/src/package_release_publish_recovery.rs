//! Retained-run recovery admission and identity verification.

use std::path::{Path, PathBuf};

use velnor_actions_contract::{
    PackageReleasePlan, PackageReleasePolicy,
    package_release::{PackageReleaseChannel, PackageReleaseReceipt},
};

use crate::{
    OrchestratorError,
    internal::internal,
    package_release_signer::{
        PackageReleaseSigningIdentity, VerifiedAttestedSignedPackage,
        verify_attested_signed_package,
    },
};
use velnor_actions_mise::CheckDeadline;

use super::{
    GitHubPackagePublisher, OriginalVerification, current_ci_from_env,
    github::{GitHubApi, WorkflowJobList, WorkflowMetadata, WorkflowRunResponse},
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

pub(super) fn recover(
    publication_deadline: CheckDeadline,
) -> Result<super::PackagePublicationOutcome, OrchestratorError> {
    let mut backend = super::artifact_zip::within_operation(|| {
        let (signed, mode) = verified_original(Purpose::PublicationRecovery, publication_deadline)?;
        let current_ci = current_ci_from_env()?;
        GitHubPackagePublisher::new(signed, current_ci, mode)
    })?;
    publish_backend(&mut backend)
}

/// Authenticate original bytes for promotion without release-write authority.
pub(crate) fn verify_original_for_promotion()
-> Result<VerifiedAttestedSignedPackage, OrchestratorError> {
    super::artifact_zip::within_operation(|| {
        let deadline = CheckDeadline::from_start_with_cleanup_reserve(
            std::time::Instant::now(),
            std::time::Duration::from_secs(
                u64::from(velnor_actions_contract::workflow::JobTimeout::DEFAULT_MINUTES) * 60,
            ),
        )
        .map_err(|_| internal("package_verification_deadline"))?;
        let (signed, _) = verified_original(Purpose::TapPromotion, deadline)?;
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
    let original_run_id = canonical_positive_env("VELNOR_RELEASE_ORIGINAL_RUN_ID")?;
    let original_attempt = canonical_positive_env_u32("VELNOR_RELEASE_ORIGINAL_RUN_ATTEMPT")?;
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
    custody_deadline: CheckDeadline,
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
    let signed = crate::package_release_signer::verify_attested_signed_package_with_deadline(
        &input,
        &plan,
        &identity,
        custody_deadline,
    )?;
    let mode = OriginalVerification::RetainedRun {
        directory: Box::new(input),
        plan: Box::new(plan),
        identity: Box::new(identity),
    };
    Ok((signed, mode))
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

#[path = "package_release_publish_recovery_checks.rs"]
mod checks;
use checks::{
    original_archive, receipt_matches_original, recovery_policy, validate_recovery_context,
};

#[path = "package_release_publish_recovery_files.rs"]
mod recovery_files;
use recovery_files::{
    canonical_positive_env, canonical_positive_env_u32, create_fresh_directory, env_path, env_text,
    positive_env, positive_env_u32, read_regular_bounded, runner_root,
};

#[path = "package_release_publish_recovery_archive.rs"]
mod archive;

#[cfg(test)]
#[path = "package_release_publish_recovery_tests.rs"]
mod tests;
