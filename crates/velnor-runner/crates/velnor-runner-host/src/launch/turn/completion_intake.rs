//! Durable completion-message intake before queue acknowledgement.

use std::collections::BTreeSet;
use std::future::Future;
use std::time::{SystemTime, UNIX_EPOCH};

use velnor_runner_github::{InnerJob, InnerKind, Poll, RunnerReference, parse_inner_messages};

use crate::error::HostError;
use crate::journal::{AssignmentKey, ConnectionCompletionInboxEntry, ConnectionJournal};
use crate::scale_set::EnsureError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IntakeDecision {
    pub(crate) completion_only: bool,
    pub(crate) wake_cleanup: bool,
    pub(crate) session_delete_candidate: bool,
    pub(crate) started_request_ids: Vec<i64>,
}

#[derive(Default)]
struct ScopedEventRecords {
    started: Vec<i64>,
    completed: Vec<i64>,
}

impl ScopedEventRecords {
    fn record(&mut self, event: RecordedScopedEvent) {
        match event {
            RecordedScopedEvent::Started(request_id) => self.started.push(request_id),
            RecordedScopedEvent::Completed(request_id) => self.completed.push(request_id),
        }
    }

    fn append(&mut self, mut other: Self) {
        self.started.append(&mut other.started);
        self.completed.append(&mut other.completed);
    }

    fn resumable_started(mut self) -> Vec<i64> {
        self.started.sort_unstable();
        self.started.dedup();
        let completed = self.completed.into_iter().collect::<BTreeSet<_>>();
        self.started
            .into_iter()
            .filter(|request_id| !completed.contains(request_id))
            .collect()
    }
}

enum RecordedScopedEvent {
    Started(i64),
    Completed(i64),
}

/// Persist scoped Started/Completed events against their exact source receipts.
pub(super) async fn intake_and_ack_if_only_scoped<F, Fut>(
    journal: &ConnectionJournal,
    set_id: i64,
    polled: &Poll,
    acknowledge: F,
) -> Result<IntakeDecision, EnsureError>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<(), EnsureError>>,
{
    let (retried, mut events) = retry_scoped(journal).await?;
    events.append(record_scoped_events(journal, set_id, polled).await?);
    let lifecycle_only = scoped_lifecycle_only(polled);
    if lifecycle_only {
        acknowledge().await?;
    }
    Ok(IntakeDecision {
        // Assigned does not change job state.
        // Save Started and Completed events before ACK.
        completion_only: lifecycle_only,
        wake_cleanup: retried || has_completion(polled),
        session_delete_candidate: session_delete_candidate(polled),
        started_request_ids: events.resumable_started(),
    })
}

/// Try session deletion only after an empty response.
/// The journal still checks for outstanding durable work before it claims DELETE.
pub(super) fn session_delete_candidate(polled: &Poll) -> bool {
    matches!(polled, Poll::Empty)
}

fn scoped_lifecycle_only(polled: &Poll) -> bool {
    match polled {
        // An undecodable body has no proven message-kind set. Keep it on the
        // queue even though `record_scoped_events` also stores it durably.
        Poll::Quarantined(_) => false,
        Poll::Batch(batch) => lifecycle_jobs_only(batch.message_id, &batch.jobs),
        Poll::Empty => false,
    }
}

fn lifecycle_jobs_only(message_id: i64, jobs: &[InnerJob]) -> bool {
    message_id >= 0
        && !jobs.is_empty()
        && jobs.iter().all(|job| {
            matches!(
                job.kind,
                InnerKind::Assigned | InnerKind::Started | InnerKind::Completed
            )
        })
}

async fn record_scoped_events(
    journal: &ConnectionJournal,
    set_id: i64,
    polled: &Poll,
) -> Result<ScopedEventRecords, EnsureError> {
    let (message_id, raw_body, jobs) = match polled {
        Poll::Batch(batch) => (
            batch.message_id,
            batch.raw_body.as_str(),
            batch.jobs.as_slice(),
        ),
        Poll::Quarantined(batch) => {
            validate_inbox_message(batch.message_id, &batch.raw_body)?;
            journal
                .store_completion_inbox(batch.message_id, &batch.raw_body)
                .await
                .map_err(map_completion_journal)?;
            return Ok(ScopedEventRecords::default());
        }
        Poll::Empty => return Ok(ScopedEventRecords::default()),
    };
    if jobs.len() > velnor_runner_github::MAX_POLL_MESSAGES {
        return Err(completion_error());
    }
    if !jobs.iter().any(is_scoped_event) {
        return Ok(ScopedEventRecords::default());
    }
    validate_inbox_message(message_id, raw_body)?;
    let mut matched = true;
    let mut events = ScopedEventRecords::default();
    for job in jobs.iter().filter(|job| is_scoped_event(job)) {
        match record_scoped_event(journal, set_id, message_id, job).await {
            Ok(Some(event)) => events.record(event),
            Ok(None) => {}
            Err(_) => matched = false,
        }
    }
    if matched && lifecycle_jobs_only(message_id, jobs) {
        journal
            .resolve_completion_inbox(&scoped_entry(journal, set_id, message_id, raw_body))
            .await
            .map_err(map_completion_journal)?;
        Ok(events)
    } else {
        journal
            .store_completion_inbox(message_id, raw_body)
            .await
            .map_err(map_completion_journal)?;
        // Keep each exact Started identity whose journal record committed,
        // even when another message keeps the response body on the inbox.
        Ok(events)
    }
}

async fn record_scoped_event(
    journal: &ConnectionJournal,
    set_id: i64,
    message_id: i64,
    job: &InnerJob,
) -> Result<Option<RecordedScopedEvent>, HostError> {
    let request_id = job
        .request_id
        .filter(|id| *id > 0)
        .ok_or(HostError::Identity)?;
    let identity = journal
        .assignment_identity(AssignmentKey::AssignedRequest { request_id })
        .await?;
    let runner = event_runner(job, set_id)?;
    let started_request_id = match job.kind {
        InnerKind::Started => {
            let runner = runner.as_ref().ok_or(HostError::Identity)?;
            journal
                .record_job_started(
                    &identity,
                    message_id,
                    &job.source,
                    runner,
                    job.job_id.as_deref(),
                )
                .await?;
            Some(RecordedScopedEvent::Started(request_id))
        }
        InnerKind::Completed => {
            journal
                .record_job_completed(
                    &identity,
                    message_id,
                    &job.source,
                    job.result.as_deref(),
                    runner.as_ref(),
                    job.job_id.as_deref(),
                )
                .await?;
            Some(RecordedScopedEvent::Completed(request_id))
        }
        InnerKind::Available | InnerKind::Assigned | InnerKind::Unsupported(_) => {
            return Err(HostError::Identity);
        }
    };
    Ok(started_request_id)
}

fn event_runner(job: &InnerJob, set_id: i64) -> Result<Option<RunnerReference>, HostError> {
    match (job.runner_id, job.runner_name.as_deref()) {
        (None, None) => Ok(None),
        (Some(id), Some(name)) => Ok(Some(RunnerReference {
            id,
            name: name.to_owned(),
            runner_scale_set_id: set_id,
        })),
        _ => Err(HostError::Identity),
    }
}

fn is_scoped_event(job: &InnerJob) -> bool {
    matches!(job.kind, InnerKind::Started | InnerKind::Completed)
}

async fn retry_scoped(
    journal: &ConnectionJournal,
) -> Result<(bool, ScopedEventRecords), EnsureError> {
    let now = unix_seconds().ok_or_else(completion_error)?;
    let due = journal
        .pending_completion_inbox(now, crate::journal::MAX_COMPLETION_INBOX_SCAN)
        .await
        .map_err(map_completion_journal)?;
    let mut resolved_any = false;
    let mut events = ScopedEventRecords::default();
    for entry in due {
        let (resolved, recorded) = retry_scoped_one(journal, &entry, now).await?;
        resolved_any |= resolved;
        events.append(recorded);
    }
    Ok((resolved_any, events))
}

async fn retry_scoped_one(
    journal: &ConnectionJournal,
    entry: &ConnectionCompletionInboxEntry,
    now: i64,
) -> Result<(bool, ScopedEventRecords), EnsureError> {
    let Ok(jobs) = parse_inner_messages(&entry.raw_body) else {
        journal
            .defer_completion_inbox(entry, now)
            .await
            .map_err(map_completion_journal)?;
        return Ok((false, ScopedEventRecords::default()));
    };
    if jobs.len() > velnor_runner_github::MAX_POLL_MESSAGES {
        journal
            .defer_completion_inbox(entry, now)
            .await
            .map_err(map_completion_journal)?;
        return Ok((false, ScopedEventRecords::default()));
    }
    if !lifecycle_jobs_only(entry.message_id, &jobs) {
        journal
            .defer_completion_inbox(entry, now)
            .await
            .map_err(map_completion_journal)?;
        return Ok((false, ScopedEventRecords::default()));
    }
    let mut matched = jobs.iter().any(is_scoped_event);
    let mut events = ScopedEventRecords::default();
    for job in jobs.iter().filter(|job| is_scoped_event(job)) {
        match record_scoped_event(journal, entry.scale_set_id, entry.message_id, job).await {
            Ok(Some(event)) => events.record(event),
            Ok(None) => {}
            Err(_) => matched = false,
        }
    }
    if matched {
        journal
            .resolve_completion_inbox(entry)
            .await
            .map_err(map_completion_journal)?;
        Ok((true, events))
    } else {
        journal
            .defer_completion_inbox(entry, now)
            .await
            .map_err(map_completion_journal)?;
        Ok((false, events))
    }
}

fn scoped_entry(
    journal: &ConnectionJournal,
    set_id: i64,
    message_id: i64,
    raw_body: &str,
) -> ConnectionCompletionInboxEntry {
    ConnectionCompletionInboxEntry {
        connection_id: journal.connection_id().clone(),
        scale_set_id: set_id,
        message_id,
        raw_body: raw_body.to_owned(),
        attempts: 0,
    }
}

fn has_completion(polled: &Poll) -> bool {
    matches!(polled, Poll::Batch(batch) if batch.jobs.iter().any(|job| matches!(job.kind, InnerKind::Completed)))
}

pub(super) fn completion_error() -> EnsureError {
    EnsureError::Unexpected {
        status: 0,
        step: "completion cleanup",
    }
}

fn map_completion_journal(error: HostError) -> EnsureError {
    match error {
        HostError::Endpoint => EnsureError::Endpoint,
        _ => EnsureError::Unexpected {
            status: 0,
            step: "completion journal",
        },
    }
}

fn validate_inbox_message(message_id: i64, raw_body: &str) -> Result<(), EnsureError> {
    if message_id < 0
        || raw_body.is_empty()
        || raw_body.len() > crate::journal::MAX_COMPLETION_BODY_BYTES
    {
        return Err(completion_error());
    }
    Ok(())
}

fn unix_seconds() -> Option<i64> {
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    i64::try_from(seconds).ok()
}
