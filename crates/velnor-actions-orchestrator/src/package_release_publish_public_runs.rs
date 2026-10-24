//! Read-only verification of public release workflow and historical recovery provenance.

use run_identity::RunSource;
use velnor_actions_contract::{PackageReleasePlan, package_release::PackageReleaseReceipt};
use velnor_actions_mise::CheckDeadline;

use crate::{OrchestratorError, internal::internal};

use super::{GitHubApi, WorkflowMetadata, WorkflowRunResponse, github, run_identity};

pub(crate) fn verify_public_release_runs(
    repository: &str,
    repository_id: u64,
    default_branch: &str,
    receipt: &PackageReleaseReceipt,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    verify_public_release_runs_inner(
        repository,
        repository_id,
        default_branch,
        receipt,
        None,
        deadline,
    )
}

pub(crate) fn verify_public_historical_release_recovery(
    repository: &str,
    repository_id: u64,
    default_branch: &str,
    receipt: &PackageReleaseReceipt,
    plan: &PackageReleasePlan,
    recovery: &crate::package_release_public::HistoricalPackageReleaseRecovery,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    verify_public_release_runs_inner(
        repository,
        repository_id,
        default_branch,
        receipt,
        Some((plan, recovery)),
        deadline,
    )
}

fn verify_public_release_runs_inner(
    repository: &str,
    repository_id: u64,
    default_branch: &str,
    receipt: &PackageReleaseReceipt,
    recovery: Option<(
        &PackageReleasePlan,
        &crate::package_release_public::HistoricalPackageReleaseRecovery,
    )>,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    if receipt.repository.slug != repository || receipt.repository.id != repository_id {
        return Err(internal("package_public_release_repository_mismatch"));
    }
    let api = GitHubApi::new(repository.to_owned()).with_deadline(deadline);
    api.verify_preview_repository_identity(repository_id, default_branch)?;
    let workflow = api.admitted_workflow(
        receipt.channel.workflow_path(),
        receipt.channel.workflow_name(),
    )?;
    let run = api.workflow_run(receipt.workflow.run_id, receipt.workflow.run_attempt)?;
    if !public_release_workflow_matches(
        &run,
        &workflow,
        receipt.workflow.run_id,
        receipt.workflow.run_attempt,
        RunSource {
            repository_id,
            sha: &receipt.workflow.authority_sha,
            branch: &receipt.source.default_branch,
        },
        receipt.channel.event().as_str(),
    ) || run.status.as_deref() != Some("completed")
        || recovery.is_some_and(|_| run.conclusion.as_deref() != Some("cancelled"))
    {
        return Err(internal(
            "package_public_release_workflow_identity_mismatch",
        ));
    }
    let producer_jobs = api
        .complete_workflow_jobs(receipt.workflow.run_id, receipt.workflow.run_attempt)?
        .jobs;
    if let Some((_, recovery)) = recovery {
        super::public_recovery::verify_public_recovery_original_jobs(
            &producer_jobs,
            receipt.workflow.run_id,
            &receipt.workflow.authority_sha,
            receipt.channel.assemble_job_id(),
            &receipt.workflow.signer_job_id,
            recovery.original_publisher_job_id,
        )?;
    } else {
        verify_public_release_producer_jobs(
            &producer_jobs,
            receipt.workflow.run_id,
            &receipt.workflow.authority_sha,
            receipt.channel.assemble_job_id(),
            &receipt.workflow.signer_job_id,
        )?;
    }
    api.verify_ci_identity(
        receipt.source.ci_run_id,
        receipt.source.ci_run_attempt,
        RunSource {
            repository_id,
            sha: &receipt.source.commit_sha,
            branch: &receipt.source.default_branch,
        },
        false,
    )?;
    if let Some((plan, recovery)) = recovery {
        super::public_recovery::verify_public_recovery_admission(
            &super::public_recovery::RecoveryAdmissionContext {
                api: &api,
                workflow: &workflow,
                repository_id,
                default_branch,
                receipt,
                plan,
                recovery,
                deadline,
            },
        )?;
    }
    Ok(())
}

pub(super) fn public_release_workflow_matches(
    run: &WorkflowRunResponse,
    workflow: &WorkflowMetadata,
    run_id: u64,
    run_attempt: u32,
    source: RunSource<'_>,
    event: &str,
) -> bool {
    run_identity::run_matches(run, workflow, run_id, run_attempt, source, event)
}

pub(super) fn verify_public_release_producer_jobs(
    jobs: &[github::WorkflowJob],
    run_id: u64,
    authority_sha: &str,
    assemble_job: &str,
    signer_job: &str,
) -> Result<(), OrchestratorError> {
    let expected = [
        assemble_job,
        signer_job,
        "Publish authenticated channel release",
    ];
    if jobs
        .iter()
        .filter(|job| expected.contains(&job.name.as_str()))
        .count()
        != expected.len()
        || expected.iter().any(|name| {
            let matching = jobs
                .iter()
                .filter(|job| job.name == *name)
                .collect::<Vec<_>>();
            matching.len() != 1
                || matching[0].run_id != run_id
                || matching[0].head_sha.as_deref() != Some(authority_sha)
                || matching[0].status.as_deref() != Some("completed")
                || matching[0].conclusion.as_deref() != Some("success")
        })
    {
        return Err(internal("package_public_release_producer_job_unsuccessful"));
    }
    Ok(())
}
