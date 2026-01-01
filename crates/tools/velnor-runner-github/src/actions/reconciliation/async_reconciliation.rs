//! Async reconciliation over the host's bounded Actions API transport.

use super::attempts::{AttemptLookup, RunnerIdentity, locate_observed_job_async};
use super::read::{Read, read_workflow_run_async};
use super::{
    ActionsJobReconciliation, ActionsJobReconciliationReason, ActionsJobReconciliationState,
    Finding, MAX_ATTEMPTS, ObservedScaleSetJob, ReconciliationContext, source_repository_finding,
    valid_runner_name,
};
use crate::actions::validate_repository;
use crate::registration::AsyncDiscoveryTransport;
use crate::{SessionError, WireError};

/// Asynchronously reconcile an observed runner lifecycle event to one exact
/// Actions job using the host's bounded discovery transport.
///
/// This has the same identity and bounded scan rules as
/// [`super::reconcile_observed_scale_set_job`]. It performs only Actions REST
/// GETs, binds the GitHub API origin before each request, and never treats the
/// opaque Scale Set `jobId` as a REST job ID. The caller should invoke it from
/// the owner of the still-open Scale Set session; it does not ACK messages,
/// acquire jobs, or establish runner/container cleanup.
///
/// # Errors
///
/// Returns an error for invalid repository/token input, malformed REST
/// responses, transport failures, or GitHub authorization and server errors.
pub async fn reconcile_observed_scale_set_job_async<T>(
    transport: &mut T,
    owner: &str,
    repository: &str,
    observed: ObservedScaleSetJob<'_>,
    actions_token: &str,
) -> Result<ActionsJobReconciliation, SessionError>
where
    T: AsyncDiscoveryTransport + ?Sized,
{
    validate_repository(owner, repository, actions_token)?;
    let mut context = ReconciliationContext::new(observed);
    let Some(run_id) = observed.workflow_run_id.filter(|id| *id > 0) else {
        return Ok(context.finish(Finding::unknown(
            ActionsJobReconciliationReason::WorkflowRunIdMissing,
        )));
    };
    if observed.runner_id.is_none_or(|id| id <= 0) {
        return Ok(context.finish(Finding::unknown(
            ActionsJobReconciliationReason::RunnerIdMissing,
        )));
    }
    if !observed.runner_name.is_some_and(valid_runner_name) {
        return Ok(context.finish(Finding::unknown(
            ActionsJobReconciliationReason::RunnerNameMissing,
        )));
    }
    if let Some(finding) = context
        .lookup_run_async(transport, owner, repository, run_id, actions_token)
        .await?
    {
        return Ok(context.finish(finding));
    }
    if let Some(finding) = context
        .locate_job_async(transport, owner, repository, actions_token)
        .await?
    {
        return Ok(context.finish(finding));
    }
    let finding = context.job_state();
    Ok(context.finish(finding))
}

impl ReconciliationContext {
    async fn lookup_run_async<T>(
        &mut self,
        transport: &mut T,
        owner: &str,
        repository: &str,
        run_id: i64,
        token: &str,
    ) -> Result<Option<Finding>, SessionError>
    where
        T: AsyncDiscoveryTransport + ?Sized,
    {
        let Read::Found(run) =
            read_workflow_run_async(transport, owner, repository, run_id, token).await?
        else {
            return Ok(Some(Finding::new(
                ActionsJobReconciliationState::NotFound,
                ActionsJobReconciliationReason::WorkflowRunNotFound,
            )));
        };
        if run.id != run_id {
            self.workflow_run = Some(run);
            return Ok(Some(Finding::mismatch(
                ActionsJobReconciliationReason::WorkflowRunIdMismatch,
            )));
        }
        if let Some(finding) = source_repository_finding(&run, owner, repository) {
            self.workflow_run = Some(run);
            return Ok(Some(finding));
        }
        self.workflow_run = Some(run);
        Ok(None)
    }

    async fn locate_job_async<T>(
        &mut self,
        transport: &mut T,
        owner: &str,
        repository: &str,
        token: &str,
    ) -> Result<Option<Finding>, SessionError>
    where
        T: AsyncDiscoveryTransport + ?Sized,
    {
        let Some(run_id) = self.workflow_run_id else {
            return Ok(Some(Finding::unknown(
                ActionsJobReconciliationReason::WorkflowRunIdMissing,
            )));
        };
        let Some(runner_id) = self.runner_id.filter(|id| *id > 0) else {
            return Ok(Some(Finding::unknown(
                ActionsJobReconciliationReason::RunnerIdMissing,
            )));
        };
        let Some(runner_name) = self
            .runner_name
            .as_deref()
            .filter(|name| valid_runner_name(name))
        else {
            return Ok(Some(Finding::unknown(
                ActionsJobReconciliationReason::RunnerNameMissing,
            )));
        };
        let Some(run) = &self.workflow_run else {
            return Ok(Some(Finding::unknown(
                ActionsJobReconciliationReason::WorkflowRunNotFound,
            )));
        };
        let latest_attempt = u32::try_from(run.run_attempt).map_err(|_| WireError::Malformed)?;
        if latest_attempt > MAX_ATTEMPTS {
            return Ok(Some(Finding::unknown(
                ActionsJobReconciliationReason::AttemptLimitExceeded,
            )));
        }
        match locate_observed_job_async(
            transport,
            owner,
            repository,
            run_id,
            RunnerIdentity {
                id: runner_id,
                name: runner_name,
            },
            latest_attempt,
            token,
        )
        .await?
        {
            AttemptLookup::Unique { attempt, job } => {
                self.attempt = Some(attempt);
                self.job = Some(job);
                Ok(None)
            }
            AttemptLookup::NotFound => Ok(Some(Finding::new(
                ActionsJobReconciliationState::NotFound,
                ActionsJobReconciliationReason::AttemptNotFound,
            ))),
            AttemptLookup::None => Ok(Some(Finding::unknown(
                ActionsJobReconciliationReason::NoRunnerJobMatch,
            ))),
            AttemptLookup::Mismatch => Ok(Some(Finding::unknown(
                ActionsJobReconciliationReason::RunnerIdentityMismatch,
            ))),
            AttemptLookup::Multiple => Ok(Some(Finding::unknown(
                ActionsJobReconciliationReason::MultipleRunnerJobMatches,
            ))),
            AttemptLookup::Unknown(reason) => Ok(Some(Finding::unknown(reason))),
        }
    }
}
