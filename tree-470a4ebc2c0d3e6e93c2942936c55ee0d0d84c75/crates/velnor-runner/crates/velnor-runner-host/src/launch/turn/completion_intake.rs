//! Durable completion-message intake before queue acknowledgement.

use std::future::Future;
use std::time::{SystemTime, UNIX_EPOCH};

use velnor_runner_github::{InnerKind, Poll, parse_inner_messages};

use crate::error::HostError;
use crate::journal::Journal;
use crate::scale_set::EnsureError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct IntakeDecision {
    pub(super) completion_only: bool,
    pub(super) wake_cleanup: bool,
}

pub(super) async fn intake_poll(
    journal: &Journal,
    set_id: i64,
    polled: &Poll,
) -> Result<IntakeDecision, EnsureError> {
    let retried = retry_quarantined(journal).await?;
    record_completed(journal, set_id, polled).await?;
    Ok(IntakeDecision {
        completion_only: completion_only(polled),
        wake_cleanup: retried || has_completion(polled),
    })
}

pub(super) async fn intake_and_ack_if_only<F, Fut>(
    journal: &Journal,
    set_id: i64,
    polled: &Poll,
    acknowledge: F,
) -> Result<IntakeDecision, EnsureError>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<(), EnsureError>>,
{
    let decision = intake_poll(journal, set_id, polled).await?;
    if decision.completion_only {
        acknowledge().await?;
    }
    Ok(decision)
}

pub(super) async fn record_completed(
    journal: &Journal,
    set_id: i64,
    polled: &Poll,
) -> Result<(), EnsureError> {
    let batch = match polled {
        Poll::Batch(batch) => {
            if batch
                .jobs
                .iter()
                .any(|job| matches!(job.kind, InnerKind::Completed))
            {
                validate_inbox_message(batch.message_id, &batch.raw_body)?;
            }
            batch
        }
        Poll::Quarantined(batch) => {
            validate_inbox_message(batch.message_id, &batch.raw_body)?;
            journal
                .store_completion_inbox(set_id, batch.message_id, &batch.raw_body)
                .await
                .map_err(map_completion_journal)?;
            return Ok(());
        }
        Poll::Empty => return Ok(()),
    };
    let completions = batch
        .jobs
        .iter()
        .filter(|job| matches!(job.kind, InnerKind::Completed));
    if batch.jobs.len() > velnor_runner_github::MAX_POLL_MESSAGES {
        return Err(completion_error());
    }
    let mut unmatched = false;
    let mut has_completion = false;
    for job in completions {
        has_completion = true;
        let Some((request_id, runner_id, runner_name)) = job
            .request_id
            .zip(job.runner_id)
            .zip(job.runner_name.as_deref())
            .map(|((request_id, runner_id), runner_name)| (request_id, runner_id, runner_name))
        else {
            unmatched = true;
            continue;
        };
        if !valid_completion_identity(request_id, runner_id, runner_name) {
            unmatched = true;
            continue;
        }
        if journal
            .record_runner_completed(set_id, request_id, runner_id, runner_name)
            .await
            .map_err(map_completion_journal)?
            .is_none()
        {
            unmatched = true;
        }
    }
    if !has_completion {
        return Ok(());
    }
    if unmatched {
        journal
            .store_completion_inbox(set_id, batch.message_id, &batch.raw_body)
            .await
            .map_err(map_completion_journal)?;
        return Ok(());
    }
    let entry = crate::journal::CompletionInboxEntry {
        scale_set_id: set_id,
        message_id: batch.message_id,
        raw_body: batch.raw_body.clone(),
        attempts: 0,
    };
    journal
        .resolve_completion_inbox(&entry)
        .await
        .map_err(map_completion_journal)?;
    Ok(())
}

/// Retry a bounded prefix of quarantined bodies without network or cleanup effects.
pub(super) async fn retry_quarantined(journal: &Journal) -> Result<bool, EnsureError> {
    let now = unix_seconds().ok_or_else(completion_error)?;
    let due = journal
        .pending_completion_inbox(now, crate::journal::MAX_COMPLETION_INBOX_SCAN)
        .await
        .map_err(map_completion_journal)?;
    let mut resolved_any = false;
    for entry in due {
        if retry_one(journal, &entry, now).await? {
            resolved_any = true;
        }
    }
    Ok(resolved_any)
}

async fn retry_one(
    journal: &Journal,
    entry: &crate::journal::CompletionInboxEntry,
    now: i64,
) -> Result<bool, EnsureError> {
    let Ok(jobs) = parse_inner_messages(&entry.raw_body) else {
        journal
            .defer_completion_inbox(entry, now)
            .await
            .map_err(map_completion_journal)?;
        return Ok(false);
    };
    if jobs.len() > velnor_runner_github::MAX_POLL_MESSAGES {
        journal
            .defer_completion_inbox(entry, now)
            .await
            .map_err(map_completion_journal)?;
        return Ok(false);
    }
    let mut has_completion = false;
    let mut unmatched = false;
    for job in jobs
        .iter()
        .filter(|job| matches!(job.kind, InnerKind::Completed))
    {
        has_completion = true;
        let Some((request_id, runner_id, runner_name)) = job
            .request_id
            .zip(job.runner_id)
            .zip(job.runner_name.as_deref())
            .map(|((request_id, runner_id), runner_name)| (request_id, runner_id, runner_name))
        else {
            unmatched = true;
            continue;
        };
        if !valid_completion_identity(request_id, runner_id, runner_name) {
            unmatched = true;
            continue;
        }
        if journal
            .record_runner_completed(entry.scale_set_id, request_id, runner_id, runner_name)
            .await
            .map_err(map_completion_journal)?
            .is_none()
        {
            unmatched = true;
        }
    }
    if !has_completion || unmatched {
        journal
            .defer_completion_inbox(entry, now)
            .await
            .map_err(map_completion_journal)?;
        return Ok(false);
    }
    journal
        .resolve_completion_inbox(entry)
        .await
        .map_err(map_completion_journal)?;
    Ok(true)
}

pub(super) fn has_completion(polled: &Poll) -> bool {
    matches!(polled, Poll::Batch(batch) if batch.jobs.iter().any(|job| matches!(job.kind, InnerKind::Completed)))
}

pub(super) fn completion_only(polled: &Poll) -> bool {
    match polled {
        Poll::Quarantined(batch) => batch.message_id >= 0,
        Poll::Batch(batch) => {
            batch.message_id >= 0
                && !batch.jobs.is_empty()
                && batch
                    .jobs
                    .iter()
                    .all(|job| matches!(job.kind, InnerKind::Completed))
        }
        Poll::Empty => false,
    }
}

fn valid_completion_identity(request_id: i64, runner_id: i64, runner_name: &str) -> bool {
    request_id > 0
        && runner_id > 0
        && !runner_name.is_empty()
        && !runner_name
            .chars()
            .any(|character| matches!(character, '\'' | '"'))
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
