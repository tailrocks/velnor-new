//! Historical public release recovery provenance predicates.

use velnor_actions_contract::{PackageReleasePlan, package_release::PackageReleaseReceipt};
use velnor_actions_mise::CheckDeadline;

use crate::{OrchestratorError, internal::internal};

use super::{GitHubApi, WorkflowMetadata, github, run_identity::RunSource, sha256};

pub(super) struct RecoveryAdmissionContext<'a> {
    pub(super) api: &'a GitHubApi,
    pub(super) workflow: &'a WorkflowMetadata,
    pub(super) repository_id: u64,
    pub(super) default_branch: &'a str,
    pub(super) receipt: &'a PackageReleaseReceipt,
    pub(super) plan: &'a PackageReleasePlan,
    pub(super) recovery: &'a crate::package_release_public::HistoricalPackageReleaseRecovery,
    pub(super) deadline: CheckDeadline,
}

pub(super) fn verify_public_recovery_original_jobs(
    jobs: &[github::WorkflowJob],
    run_id: u64,
    authority_sha: &str,
    assemble_job: &str,
    signer_job: &str,
    cancelled_publisher_job_id: u64,
) -> Result<(), OrchestratorError> {
    let expected = [assemble_job, signer_job];
    if expected.iter().any(|name| {
        let matching = jobs
            .iter()
            .filter(|job| job.name == *name)
            .collect::<Vec<_>>();
        matching.len() != 1
            || matching[0].run_id != run_id
            || matching[0].head_sha.as_deref() != Some(authority_sha)
            || matching[0].status.as_deref() != Some("completed")
            || matching[0].conclusion.as_deref() != Some("success")
    }) {
        return Err(internal("package_public_release_producer_job_unsuccessful"));
    }
    let publishers = jobs
        .iter()
        .filter(|job| job.name == "Publish authenticated channel release")
        .collect::<Vec<_>>();
    if publishers.len() != 1
        || publishers[0].id != cancelled_publisher_job_id
        || publishers[0].run_id != run_id
        || publishers[0].head_sha.as_deref() != Some(authority_sha)
        || publishers[0].status.as_deref() != Some("completed")
        || publishers[0].conclusion.as_deref() != Some("cancelled")
    {
        return Err(internal(
            "package_public_historical_publisher_not_cancelled",
        ));
    }
    Ok(())
}

pub(super) fn verify_public_recovery_admission(
    context: &RecoveryAdmissionContext<'_>,
) -> Result<(), OrchestratorError> {
    let recovery = context.recovery;
    let recovery_run = context
        .api
        .workflow_run(recovery.run_id, recovery.run_attempt)?;
    if !super::public_runs::public_release_workflow_matches(
        &recovery_run,
        context.workflow,
        recovery.run_id,
        recovery.run_attempt,
        RunSource {
            repository_id: context.repository_id,
            sha: &recovery.authority_sha,
            branch: context.default_branch,
        },
        "workflow_dispatch",
    ) || recovery_run.status.as_deref() != Some("completed")
    {
        return Err(internal("package_public_historical_recovery_run_mismatch"));
    }
    let jobs = context
        .api
        .complete_workflow_jobs(recovery.run_id, recovery.run_attempt)?
        .jobs;
    verify_public_recovery_jobs(&jobs, recovery)?;
    let projection = read_recovery_projection(context)?;
    let expected_plan_sha256 = sha256(
        &context
            .plan
            .canonical_bytes()
            .map_err(|_| internal("package_public_plan_invalid"))?,
    );
    if projection.repository_id != context.repository_id
        || projection.head_repository_id != context.repository_id
        || projection.source_sha != context.receipt.source.commit_sha
        || projection.source_ref != context.receipt.source.source_ref
        || projection.workflow_authority_sha != recovery.authority_sha
        || projection.run_id != recovery.run_id
        || projection.run_attempt != recovery.run_attempt
        || projection.ci_run_id != context.receipt.source.ci_run_id
        || projection.ci_run_attempt != context.receipt.source.ci_run_attempt
        || projection.commit_count != context.receipt.version.commit_count.unwrap_or_default()
        || projection.version != context.receipt.version.value
        || projection.plan_sha256 != expected_plan_sha256
        || projection.policy_sha256 != recovery.policy_sha256
    {
        return Err(internal(
            "package_public_historical_recovery_admission_mismatch",
        ));
    }
    Ok(())
}

fn read_recovery_projection(
    context: &RecoveryAdmissionContext<'_>,
) -> Result<
    crate::package_release_publish::release_policy_admission::PublicHistoricalAdmissionProjection,
    OrchestratorError,
> {
    let zip = read_verified_recovery_archive(context)?;
    let recovery = context.recovery;
    let staging =
        tempfile::tempdir().map_err(|_| internal("package_public_historical_recovery_staging"))?;
    let entry = crate::package_release_publish::artifact_zip::ExpectedZipEntry {
        name: "preview-source-admission.json".to_owned(),
        size: Some(recovery.admission_size),
        sha256: Some(recovery.admission_sha256.clone()),
        max_bytes: 16 * 1024 * 1024,
        unix_permissions: Some(0o644),
    };
    context
        .deadline
        .scope(|| {
            crate::package_release_publish::artifact_zip::extract_in_scope(
                &zip,
                staging.path(),
                &[entry],
            )
        })
        .map_err(|_| internal("package_public_verification_deadline"))??;
    let admission = crate::retrieve_reports::staged_reads::read_staged_bytes(
        &staging.path().join("preview-source-admission.json"),
        16 * 1024 * 1024,
    )
    .map_err(|_| internal("package_public_historical_recovery_admission_read"))?;
    crate::package_release_publish::release_policy_admission::project_public_historical_admission(
        &admission,
        &recovery.admission_sha256,
    )
}

fn read_verified_recovery_archive(
    context: &RecoveryAdmissionContext<'_>,
) -> Result<Vec<u8>, OrchestratorError> {
    let recovery = context.recovery;
    let artifact_matches = context
        .api
        .workflow_artifacts(recovery.run_id)?
        .into_iter()
        .filter(|artifact| {
            artifact.id == recovery.artifact_id || artifact.name == recovery.artifact_name
        })
        .collect::<Vec<_>>();
    if artifact_matches.len() != 1 {
        return Err(internal(
            "package_public_historical_recovery_artifact_duplicate",
        ));
    }
    let artifact = &artifact_matches[0];
    validate_recovery_artifact(artifact, context)?;
    context
        .deadline
        .remaining()
        .map_err(|_| internal("package_public_verification_deadline"))?;
    let zip = context.api.download_artifact_archive_bounded(
        recovery.artifact_id,
        usize::try_from(artifact.size_in_bytes)
            .map_err(|_| internal("package_public_historical_recovery_artifact_size"))?,
    )?;
    if zip.len() as u64 != artifact.size_in_bytes
        || artifact
            .digest
            .as_ref()
            .is_some_and(|digest| digest != &format!("sha256:{}", sha256(&zip)))
    {
        return Err(internal(
            "package_public_historical_recovery_artifact_digest",
        ));
    }
    Ok(zip)
}

fn validate_recovery_artifact(
    artifact: &github::WorkflowArtifact,
    context: &RecoveryAdmissionContext<'_>,
) -> Result<(), OrchestratorError> {
    let recovery = context.recovery;
    let artifact_run = artifact
        .workflow_run
        .as_ref()
        .ok_or_else(|| internal("package_public_historical_recovery_artifact_run"))?;
    if artifact.id != recovery.artifact_id
        || artifact.name != recovery.artifact_name
        || artifact.expired
        || artifact.size_in_bytes == 0
        || artifact.size_in_bytes > 32 * 1024 * 1024
        || artifact_run.id != recovery.run_id
        || artifact_run.repository_id != context.repository_id
        || artifact_run.head_sha != recovery.authority_sha
        || artifact_run.head_branch != context.default_branch
    {
        return Err(internal(
            "package_public_historical_recovery_artifact_mismatch",
        ));
    }
    Ok(())
}

pub(super) fn verify_public_recovery_jobs(
    jobs: &[github::WorkflowJob],
    recovery: &crate::package_release_public::HistoricalPackageReleaseRecovery,
) -> Result<(), OrchestratorError> {
    let matching_job = |name: &str| {
        jobs.iter()
            .filter(|job| job.name == name)
            .collect::<Vec<_>>()
    };
    let recovery_jobs = matching_job("Recover the original signed release without rebuilding");
    let source_jobs = matching_job("Select authenticated main source for preview");
    if recovery_jobs.len() != 1
        || recovery_jobs[0].id != recovery.recovery_job_id
        || recovery_jobs[0].run_id != recovery.run_id
        || recovery_jobs[0].head_sha.as_deref() != Some(&recovery.authority_sha)
        || recovery_jobs[0].status.as_deref() != Some("completed")
        || recovery_jobs[0].conclusion.as_deref() != Some("success")
        || source_jobs.len() != 1
        || source_jobs[0].run_id != recovery.run_id
        || source_jobs[0].head_sha.as_deref() != Some(&recovery.authority_sha)
        || source_jobs[0].status.as_deref() != Some("completed")
        || source_jobs[0].conclusion.as_deref() != Some("success")
    {
        return Err(internal("package_public_historical_recovery_job_mismatch"));
    }
    for name in [
        "preview-assemble",
        "preview-signer",
        "Publish authenticated channel release",
    ] {
        let matching = matching_job(name);
        if matching.len() != 1
            || matching[0].run_id != recovery.run_id
            || matching[0].head_sha.as_deref() != Some(&recovery.authority_sha)
            || matching[0].status.as_deref() != Some("completed")
            || matching[0].conclusion.as_deref() != Some("skipped")
        {
            return Err(internal("package_public_historical_recovery_rebuilt"));
        }
    }
    let build_jobs = jobs
        .iter()
        .filter(|job| job.name.starts_with("Build "))
        .collect::<Vec<_>>();
    if build_jobs.is_empty()
        || build_jobs.iter().any(|job| {
            job.run_id != recovery.run_id
                || job.head_sha.as_deref() != Some(&recovery.authority_sha)
                || job.status.as_deref() != Some("completed")
                || job.conclusion.as_deref() != Some("skipped")
        })
    {
        return Err(internal("package_public_historical_recovery_rebuilt"));
    }
    Ok(())
}
