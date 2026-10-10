//! Completion inbox bounds, replay, and exact-body tests.

use crate::error::HostError;
use crate::journal::MAX_COMPLETION_BODY_BYTES;

const EXISTING_BACKLOG_ROWS: i64 = 128;
use crate::launch_harness::open;

#[tokio::test]
async fn exact_body_is_idempotent_and_resolves_after_retry() -> Result<(), String> {
    let (_scratch, journal) = open("completion-inbox-exact").await?;
    let body = r#"[{"messageType":"JobCompleted","runnerRequestId":61,"runnerId":901,"runnerName":"v61"}]"#;
    journal
        .store_completion_inbox(7, 50, body)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .store_completion_inbox(7, 50, body)
        .await
        .map_err(|error| error.to_string())?;
    let pending = journal
        .pending_completion_inbox(0, 4)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].raw_body, body);
    assert_eq!(pending[0].attempts, 0);

    journal
        .defer_completion_inbox(&pending[0], 10)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        journal
            .pending_completion_inbox(10, 4)
            .await
            .map_err(|error| error.to_string())?,
        [] as [crate::journal::CompletionInboxEntry; 0]
    );
    let retry = journal
        .pending_completion_inbox(11, 4)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(retry.len(), 1);
    assert_eq!(retry[0].attempts, 1);
    journal
        .resolve_completion_inbox(&retry[0])
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        journal
            .pending_completion_inbox(11, 4)
            .await
            .map_err(|error| error.to_string())?,
        [] as [crate::journal::CompletionInboxEntry; 0]
    );
    Ok(())
}

#[tokio::test]
async fn conflicting_body_for_same_message_is_rejected() -> Result<(), String> {
    let (_scratch, journal) = open("completion-inbox-conflict").await?;
    journal
        .store_completion_inbox(7, 50, "[]")
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        journal.store_completion_inbox(7, 50, "[ ]").await,
        Err(HostError::Journal)
    );
    Ok(())
}

#[tokio::test]
async fn missing_id_and_oversized_body_fail_closed() -> Result<(), String> {
    let (_scratch, journal) = open("completion-inbox-invalid").await?;
    assert_eq!(
        journal.store_completion_inbox(7, -1, "[]").await,
        Err(HostError::Journal)
    );
    assert_eq!(
        journal.store_completion_inbox(0, 50, "[]").await,
        Err(HostError::Journal)
    );
    let oversized = "x".repeat(MAX_COMPLETION_BODY_BYTES + 1);
    assert_eq!(
        journal.store_completion_inbox(7, 50, &oversized).await,
        Err(HostError::Journal)
    );
    Ok(())
}

#[tokio::test]
async fn inbox_persists_overflow_beyond_bounded_retry_page() -> Result<(), String> {
    let (_scratch, journal) = open("completion-inbox-full").await?;
    for message_id in 0..=EXISTING_BACKLOG_ROWS {
        journal
            .store_completion_inbox(7, message_id, "[]")
            .await
            .map_err(|error| error.to_string())?;
    }
    let pending = journal
        .pending_completion_inbox(0, u32::MAX)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(pending.len(), 4);
    assert_eq!(pending[3].message_id, 3);
    journal
        .store_completion_inbox(7, EXISTING_BACKLOG_ROWS, "[]")
        .await
        .map_err(|error| error.to_string())?;
    assert!(
        journal
            .store_completion_inbox(7, EXISTING_BACKLOG_ROWS, "different")
            .await
            .is_err()
    );
    assert_eq!(
        journal
            .pending_completion_inbox(0, u32::MAX)
            .await
            .map_err(|error| error.to_string())?
            .len(),
        4
    );
    Ok(())
}
