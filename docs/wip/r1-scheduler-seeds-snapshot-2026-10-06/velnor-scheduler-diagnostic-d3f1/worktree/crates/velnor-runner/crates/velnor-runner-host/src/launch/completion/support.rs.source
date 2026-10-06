use std::time::{SystemTime, UNIX_EPOCH};

use crate::action_archive_seed::ActionArchiveStore;
use crate::error::HostError;
use crate::journal::Journal;
use crate::reconcile::IntentRow;
use crate::scale_set::EnsureError;

pub(super) fn runner_matches(
    runner: &velnor_runner_github::RunnerReference,
    runner_id: i64,
    set_id: i64,
    runner_name: &str,
) -> bool {
    runner.id == runner_id && runner.runner_scale_set_id == set_id && runner.name == runner_name
}

pub(super) fn map_journal(error: HostError) -> EnsureError {
    match error {
        HostError::Endpoint => EnsureError::Endpoint,
        _ => EnsureError::Unexpected {
            status: 0,
            step: "journal",
        },
    }
}

pub(super) fn completion_error() -> EnsureError {
    EnsureError::Unexpected {
        status: 0,
        step: "completion-event",
    }
}

pub(super) fn held(row: &IntentRow, stage: &str) -> Result<bool, EnsureError> {
    if std::env::var_os("VELNOR_HTTPS_TRACE").is_some() {
        eprintln!(
            "completion_cleanup=hold launch_id={} stage={stage}",
            row.launch_id.as_deref().unwrap_or("-")
        );
    }
    Ok(false)
}

pub(super) fn unix_seconds() -> Result<i64, EnsureError> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| completion_error())?
        .as_secs();
    i64::try_from(seconds).map_err(|_| completion_error())
}

pub(super) fn retry_at(attempt: u32) -> Result<i64, EnsureError> {
    let delay = 2_u64.saturating_pow(attempt).min(30);
    unix_seconds()?
        .checked_add(i64::try_from(delay).map_err(|_| completion_error())?)
        .ok_or_else(completion_error)
}

pub(super) fn open_archive_store(journal: &Journal) -> Result<ActionArchiveStore, EnsureError> {
    // The scheduler archive root is the `action-archives` sibling of the canonical journal.
    // The publisher must use the same root. Recovery must not create a missing store.
    let root = journal
        .path()
        .parent()
        .ok_or_else(completion_error)?
        .join("action-archives");
    ActionArchiveStore::open_existing(root).map_err(|_| completion_error())
}

pub(super) fn log_cleanup_error(row: &IntentRow, stage: &str, detail: &str) {
    if std::env::var_os("VELNOR_HTTPS_TRACE").is_some() {
        eprintln!(
            "completion_cleanup=error launch_id={} stage={stage} detail={detail}",
            row.launch_id.as_deref().unwrap_or("-")
        );
    }
}
