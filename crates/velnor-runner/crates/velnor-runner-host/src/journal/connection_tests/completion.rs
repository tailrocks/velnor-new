//! Each completion inbox identity belongs to one repository connection.

#[path = "completion_claim_tests.rs"]
mod completion_claim_tests;

use super::*;

#[tokio::test]
async fn completion_inboxes_are_scoped_and_survive_restart() -> Result<(), HostError> {
    let scratch = Scratch::new("completion-scope")?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    let first = scoped(&journal, "essential", 101, "donbeave/essential-mac").await?;
    let second = scoped(&journal, "chainargos", 202, "ChainArgos/java-monorepo").await?;
    let inbox_only = scoped(&journal, "observer", 303, "donbeave/observer").await?;

    exercise_inbox_only(&inbox_only).await?;
    for scope in [&first, &second] {
        scope.store_completion_inbox(91, "same-body").await?;
        scope.store_completion_inbox(91, "same-body").await?;
    }
    let first_inbox = first.pending_completion_inbox(0, 8).await?;
    let second_inbox = second.pending_completion_inbox(0, 8).await?;
    assert_eq!(first_inbox.len(), 1);
    assert_eq!(second_inbox.len(), 1);
    assert_eq!(first_inbox[0].message_id, second_inbox[0].message_id);
    assert_eq!(first_inbox[0].raw_body, second_inbox[0].raw_body);

    drop(first);
    drop(second);
    drop(inbox_only);
    drop(journal);

    let journal = Journal::open(&path).await?;
    let first = scoped(&journal, "essential", 101, "donbeave/essential-mac").await?;
    let second = scoped(&journal, "chainargos", 202, "ChainArgos/java-monorepo").await?;
    assert_eq!(first.pending_completion_inbox(0, 8).await?, first_inbox);
    assert_eq!(second.pending_completion_inbox(0, 8).await?, second_inbox);
    Ok(())
}

#[tokio::test]
async fn completion_inbox_rejects_conflicts_invalid_ids_and_oversized_bodies()
-> Result<(), HostError> {
    let scratch = Scratch::new("completion-inbox-validation")?;
    let journal = Journal::open(&scratch.file()).await?;
    let scope = scoped(&journal, "essential", 101, "donbeave/essential-mac").await?;
    scope.store_completion_inbox(50, "[]").await?;
    assert_eq!(
        scope.store_completion_inbox(50, "[ ]").await,
        Err(HostError::Journal)
    );
    assert_eq!(
        scope.store_completion_inbox(-1, "[]").await,
        Err(HostError::Journal)
    );
    assert_eq!(
        scope.store_completion_inbox(51, "").await,
        Err(HostError::Journal)
    );
    let oversized = "x".repeat(crate::journal::MAX_COMPLETION_BODY_BYTES + 1);
    assert_eq!(
        scope.store_completion_inbox(52, &oversized).await,
        Err(HostError::Journal)
    );
    Ok(())
}

#[tokio::test]
async fn completion_inbox_scan_keeps_rows_beyond_its_bounded_page() -> Result<(), HostError> {
    let scratch = Scratch::new("completion-inbox-page-bound")?;
    let journal = Journal::open(&scratch.file()).await?;
    let scope = scoped(&journal, "essential", 101, "donbeave/essential-mac").await?;
    for message_id in 0..=128 {
        scope.store_completion_inbox(message_id, "[]").await?;
    }
    let page = scope.pending_completion_inbox(0, u32::MAX).await?;
    assert_eq!(page.len(), 4);
    assert_eq!(page[0].message_id, 0);
    assert_eq!(page[3].message_id, 3);
    scope.store_completion_inbox(128, "[]").await?;
    assert_eq!(
        scope.store_completion_inbox(128, "different").await,
        Err(HostError::Journal)
    );
    assert_eq!(scope.pending_completion_inbox(0, u32::MAX).await?.len(), 4);
    Ok(())
}

async fn exercise_inbox_only(scope: &ConnectionJournal) -> Result<(), HostError> {
    scope.store_completion_inbox(92, "inbox-only").await?;
    assert!(!scope.is_drained().await?);
    let inbox_entry = scope
        .pending_completion_inbox(0, 1)
        .await?
        .into_iter()
        .next()
        .ok_or(HostError::Journal)?;
    scope.defer_completion_inbox(&inbox_entry, 0).await?;
    assert_eq!(scope.pending_completion_inbox(0, 1).await?.len(), 0);
    let deferred = scope
        .pending_completion_inbox(1, 1)
        .await?
        .into_iter()
        .next()
        .ok_or(HostError::Journal)?;
    assert_eq!(deferred.attempts, 1);
    scope.resolve_completion_inbox(&deferred).await?;
    assert!(scope.is_drained().await?);
    Ok(())
}
