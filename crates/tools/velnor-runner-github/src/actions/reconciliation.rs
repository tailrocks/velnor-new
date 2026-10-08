//! Bounded reconciliation using lifecycle-observed runner identity.

mod async_reconciliation;
mod attempts;
mod read;

use attempts::{AttemptLookup, RunnerIdentity, locate_observed_job};
use read::{Read, read_workflow_run};

use crate::{ActionsJob, ActionsWorkflowRun, SessionError, Transport, WireError};

pub use async_reconciliation::reconcile_observed_scale_set_job_async;

use super::validate_repository;

const MAX_ATTEMPTS: u32 = 8;
const MAX_PAGES_PER_ATTEMPT: usize = 4;
const PAGE_SIZE: usize = 100;

/// Identity copied from one `JobStarted` or `JobCompleted` message.
///
/// Do not fill this from a `JobAvailable` request or an expected JIT runner
/// name. The Scale Set job ID stays opaque and is retained as evidence only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObservedScaleSetJob<'a> {
    /// Original opaque `jobId`, when the event supplied it.
    pub scale_set_job_id: Option<&'a str>,
    /// `workflowRunId` from the lifecycle event, not a requested-job record.
    pub workflow_run_id: Option<i64>,
    /// Actual `runnerId` from the lifecycle event.
    pub runner_id: Option<i64>,
    /// Actual `runnerName` from the lifecycle event.
    pub runner_name: Option<&'a str>,
}

/// Conservative reconciliation result for one observed lifecycle event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionsJobReconciliationState {
    /// GitHub returned 404 for the workflow run or an attempt listing.
    NotFound,
    /// IDs or the source repository disagree with the configured event.
    Mismatch,
    /// The exact job is queued or in progress in one unique attempt.
    Pending,
    /// The exact job is completed in one unique attempt.
    Completed,
    /// Required identity or complete bounded REST evidence is unavailable.
    Unknown,
}

/// Why the observed lifecycle identity did not establish a terminal job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionsJobReconciliationReason {
    /// The lifecycle observation omitted or had an invalid workflow run ID.
    WorkflowRunIdMissing,
    /// The lifecycle observation omitted or had an invalid actual runner ID.
    RunnerIdMissing,
    /// The lifecycle observation omitted or had an invalid actual runner name.
    RunnerNameMissing,
    /// GitHub returned 404 for the workflow run.
    WorkflowRunNotFound,
    /// GitHub returned 404 for an exact workflow attempt's jobs.
    AttemptNotFound,
    /// The returned workflow run ID disagreed with the observed ID.
    WorkflowRunIdMismatch,
    /// GitHub omitted the source repository identity.
    SourceRepositoryMissing,
    /// The workflow run came from a repository other than the configured one.
    SourceRepositoryMismatch,
    /// The workflow run had more attempts than the bounded scan allows.
    AttemptLimitExceeded,
    /// An attempt had more job pages than the bounded scan allows.
    PaginationLimitExceeded,
    /// A page count or page length changed during the bounded scan.
    IncompleteAttemptPage,
    /// An Actions job ID appeared more than once in one attempt listing.
    DuplicateAttemptJobId,
    /// An attempt returned a job for a different workflow run.
    AttemptRunIdMismatch,
    /// No attempt row matched both actual lifecycle runner fields.
    NoRunnerJobMatch,
    /// Only one of the actual runner ID and name matched a job row.
    RunnerIdentityMismatch,
    /// More than one exact row matched the observed runner identity.
    MultipleRunnerJobMatches,
    /// The matching row had a status outside the known REST states.
    UnknownJobStatus,
}

/// Actions REST evidence, retaining both Scale Set and REST identities.
///
/// `job.id` is the REST job ID. It is never derived from or substituted for
/// the opaque Scale Set `jobId`. `Completed` proves a remote job ended, but a
/// caller may release a local launch only after its runner/container cleanup
/// is separately verified. This does not bind a request ID to a JIT runner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionsJobReconciliation {
    /// Typed conclusion; only `Completed` is remote terminal evidence.
    pub state: ActionsJobReconciliationState,
    /// Original opaque Scale Set ID from the lifecycle event.
    pub scale_set_job_id: Option<String>,
    /// Workflow run ID from the lifecycle event.
    pub observed_workflow_run_id: Option<i64>,
    /// Actual runner ID observed in the lifecycle event.
    pub observed_runner_id: Option<i64>,
    /// Actual runner name observed in the lifecycle event.
    pub observed_runner_name: Option<String>,
    /// Unique attempt number containing the exact runner/job row, if proven.
    pub attempt: Option<u32>,
    /// Actions job REST row; `id` remains distinct from the opaque Scale Set ID.
    pub job: Option<ActionsJob>,
    /// Workflow source and run metadata from the REST API.
    pub workflow_run: Option<ActionsWorkflowRun>,
    /// Additional explanation for unresolved or mismatched results.
    pub reason: Option<ActionsJobReconciliationReason>,
}

/// Reconcile an observed runner lifecycle event to one exact Actions job.
///
/// This never interprets the Scale Set's opaque `jobId` as a REST integer.
/// It uses only the observed run ID and actual runner ID/name, requires one
/// complete attempt-specific job row matching both runner fields, and checks
/// the run's source repository. Pagination is capped at four pages of 100
/// jobs per attempt, and attempt history at eight attempts. No retries are
/// made; missing identity, 404, nonterminal state, or cap exhaustion stays
/// unresolved.
///
/// # Errors
///
/// Returns an error for invalid repository/token input, malformed REST
/// responses, transport failures, or GitHub authorization and server errors.
pub fn reconcile_observed_scale_set_job<T>(
    transport: &mut T,
    owner: &str,
    repository: &str,
    observed: ObservedScaleSetJob<'_>,
    actions_token: &str,
) -> Result<ActionsJobReconciliation, SessionError>
where
    T: Transport + ?Sized,
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
    if let Some(finding) =
        context.lookup_run(transport, owner, repository, run_id, actions_token)?
    {
        return Ok(context.finish(finding));
    }
    if let Some(finding) = context.locate_job(transport, owner, repository, actions_token)? {
        return Ok(context.finish(finding));
    }
    let finding = context.job_state();
    Ok(context.finish(finding))
}

struct ReconciliationContext {
    scale_set_job_id: Option<String>,
    workflow_run_id: Option<i64>,
    runner_id: Option<i64>,
    runner_name: Option<String>,
    attempt: Option<u32>,
    job: Option<ActionsJob>,
    workflow_run: Option<ActionsWorkflowRun>,
}

impl ReconciliationContext {
    fn new(observed: ObservedScaleSetJob<'_>) -> Self {
        Self {
            scale_set_job_id: observed.scale_set_job_id.map(ToOwned::to_owned),
            workflow_run_id: observed.workflow_run_id,
            runner_id: observed.runner_id,
            runner_name: observed.runner_name.map(ToOwned::to_owned),
            attempt: None,
            job: None,
            workflow_run: None,
        }
    }

    fn finish(self, finding: Finding) -> ActionsJobReconciliation {
        ActionsJobReconciliation {
            state: finding.state,
            scale_set_job_id: self.scale_set_job_id,
            observed_workflow_run_id: self.workflow_run_id,
            observed_runner_id: self.runner_id,
            observed_runner_name: self.runner_name,
            attempt: self.attempt,
            job: self.job,
            workflow_run: self.workflow_run,
            reason: finding.reason,
        }
    }

    fn lookup_run<T>(
        &mut self,
        transport: &mut T,
        owner: &str,
        repository: &str,
        run_id: i64,
        token: &str,
    ) -> Result<Option<Finding>, SessionError>
    where
        T: Transport + ?Sized,
    {
        let Read::Found(run) = read_workflow_run(transport, owner, repository, run_id, token)?
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

    fn locate_job<T>(
        &mut self,
        transport: &mut T,
        owner: &str,
        repository: &str,
        token: &str,
    ) -> Result<Option<Finding>, SessionError>
    where
        T: Transport + ?Sized,
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
        match locate_observed_job(
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
        )? {
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

    fn job_state(&self) -> Finding {
        match self.job.as_ref().map(|job| job.status.as_str()) {
            Some("completed") => Finding::completed(),
            Some("queued" | "in_progress") => Finding::pending(),
            _ => Finding::unknown(ActionsJobReconciliationReason::UnknownJobStatus),
        }
    }
}

#[derive(Clone, Copy)]
struct Finding {
    state: ActionsJobReconciliationState,
    reason: Option<ActionsJobReconciliationReason>,
}

impl Finding {
    fn new(state: ActionsJobReconciliationState, reason: ActionsJobReconciliationReason) -> Self {
        Self {
            state,
            reason: Some(reason),
        }
    }

    fn unknown(reason: ActionsJobReconciliationReason) -> Self {
        Self::new(ActionsJobReconciliationState::Unknown, reason)
    }

    fn mismatch(reason: ActionsJobReconciliationReason) -> Self {
        Self::new(ActionsJobReconciliationState::Mismatch, reason)
    }

    fn pending() -> Self {
        Self {
            state: ActionsJobReconciliationState::Pending,
            reason: None,
        }
    }

    fn completed() -> Self {
        Self {
            state: ActionsJobReconciliationState::Completed,
            reason: None,
        }
    }
}

fn source_repository_finding(
    workflow_run: &ActionsWorkflowRun,
    owner: &str,
    repository: &str,
) -> Option<Finding> {
    let Some(head_repository) = workflow_run.head_repository_full_name.as_deref() else {
        return Some(Finding::unknown(
            ActionsJobReconciliationReason::SourceRepositoryMissing,
        ));
    };
    if !head_repository.eq_ignore_ascii_case(&format!("{owner}/{repository}")) {
        return Some(Finding::mismatch(
            ActionsJobReconciliationReason::SourceRepositoryMismatch,
        ));
    }
    None
}

fn valid_runner_name(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
}
