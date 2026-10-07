#[cfg(test)]
use std::sync::Arc;
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};

#[cfg(test)]
use velnor_runner_host::scale_set::EnsureError;
#[cfg(test)]
use velnor_runner_journal::journal::Journal;

#[cfg(test)]
use crate::launch::harness;

#[cfg(test)]
use super::{close_after_poll, create_only_if_resolved, leaked, require_no_unresolved};

#[test]
fn held_offer_is_not_reported_as_a_clean_session_exit() {
    let outcome = crate::launch::turn::PollOutcome {
        workers: Vec::new(),
        retain_session: true,
    };
    assert_eq!(
        crate::launch::report(1, Ok(outcome), Ok(())),
        Err(EnsureError::Uncertain)
    );
}

#[tokio::test]
async fn unresolved_session_survives_restart_and_blocks_create() -> Result<(), String> {
    let (scratch, journal) = harness::open("session-quarantine-restart").await?;
    let row = journal
        .begin("session", "prior-session-id")
        .await
        .map_err(|error| error.to_string())?;
    drop(journal);

    let reopened = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        require_no_unresolved(&reopened).await,
        Err(EnsureError::Uncertain)
    );
    let unresolved = leaked(&reopened)
        .await
        .map_err(|error| format!("read session rows: {error}"))?;
    let create_calls = Arc::new(AtomicUsize::new(0));
    let called = Arc::clone(&create_calls);
    let result = create_only_if_resolved(&unresolved, || async move {
        called.fetch_add(1, Ordering::SeqCst);
        Ok(())
    })
    .await;

    assert_eq!(result, Err(EnsureError::Uncertain));
    assert_eq!(create_calls.load(Ordering::SeqCst), 0);
    let rows = reopened.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, row);
    assert_eq!(rows[0].subject, "prior-session-id");
    assert!(!rows[0].cleanup_proven);
    Ok(())
}

#[tokio::test]
async fn failed_poll_preserves_session_without_delete() {
    let poll: Result<(), EnsureError> = Err(EnsureError::Uncertain);
    let delete_calls = Arc::new(AtomicUsize::new(0));
    let called = Arc::clone(&delete_calls);
    let result = close_after_poll(&poll, false, || async move {
        called.fetch_add(1, Ordering::SeqCst);
        Ok(())
    })
    .await;

    assert_eq!(result, Ok(()));
    assert_eq!(delete_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn held_unacknowledged_offer_preserves_session_without_delete() {
    let poll: Result<(), EnsureError> = Ok(());
    let delete_calls = Arc::new(AtomicUsize::new(0));
    let called = Arc::clone(&delete_calls);
    let result = close_after_poll(&poll, true, || async move {
        called.fetch_add(1, Ordering::SeqCst);
        Ok(())
    })
    .await;

    assert_eq!(result, Ok(()));
    assert_eq!(delete_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn clean_poll_closes_session_once() {
    let poll: Result<(), EnsureError> = Ok(());
    let delete_calls = Arc::new(AtomicUsize::new(0));
    let called = Arc::clone(&delete_calls);
    let result = close_after_poll(&poll, false, || async move {
        called.fetch_add(1, Ordering::SeqCst);
        Ok(())
    })
    .await;

    assert_eq!(result, Ok(()));
    assert_eq!(delete_calls.load(Ordering::SeqCst), 1);
}
