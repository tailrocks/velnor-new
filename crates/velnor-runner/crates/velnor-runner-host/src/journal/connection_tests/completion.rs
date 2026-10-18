//! Completion identity and inbox isolation across repository connections.

use super::*;

struct CompletionFixture {
    first_launch: i64,
    second_launch: i64,
    first_inbox: Vec<crate::journal::ConnectionCompletionInboxEntry>,
    second_inbox: Vec<crate::journal::ConnectionCompletionInboxEntry>,
}

#[tokio::test]
async fn completion_and_inbox_keys_are_scoped_and_survive_restart() -> Result<(), HostError> {
    let scratch = Scratch::new("completion-scope")?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    journal
        .configure_capacity(limits(2, 4_000, 17_179_869_184))
        .await?;
    let first = scoped(&journal, "essential", 101, "donbeave/essential-mac").await?;
    let second = scoped(&journal, "chainargos", 202, "ChainArgos/java-monorepo").await?;
    let inbox_only = scoped(&journal, "observer", 303, "donbeave/observer").await?;

    exercise_inbox_only(&journal, &inbox_only).await?;
    let completion = complete_requests_in_scopes(&first, &second).await?;
    drop(first);
    drop(second);
    drop(inbox_only);
    drop(journal);

    let journal = Journal::open(&path).await?;
    let first = scoped(&journal, "essential", 101, "donbeave/essential-mac").await?;
    let second = scoped(&journal, "chainargos", 202, "ChainArgos/java-monorepo").await?;
    assert_reopened_completion_rows(&first, &second, &completion).await?;
    exercise_scoped_completion_cleanup(&first, &second, &completion).await?;
    Ok(())
}

async fn exercise_inbox_only(
    journal: &Journal,
    scope: &ConnectionJournal,
) -> Result<(), HostError> {
    scope.store_completion_inbox(92, "inbox-only").await?;
    assert!(!scope.is_drained().await?);
    assert!(!journal.is_drained().await?);
    assert!(!scope.retire_if_drained().await?);
    let inbox_entry = scope
        .pending_completion_inbox(0, 1)
        .await?
        .into_iter()
        .next()
        .ok_or(HostError::Journal)?;
    scope.defer_completion_inbox(&inbox_entry, 0).await?;
    assert!(scope.pending_completion_inbox(0, 1).await?.is_empty());
    let deferred = scope
        .pending_completion_inbox(1, 1)
        .await?
        .into_iter()
        .next()
        .ok_or(HostError::Journal)?;
    assert_eq!(deferred.attempts, 1);
    scope.resolve_completion_inbox(&deferred).await?;
    assert!(scope.is_drained().await?);
    assert!(journal.is_drained().await?);
    Ok(())
}

async fn complete_requests_in_scopes(
    first: &ConnectionJournal,
    second: &ConnectionJournal,
) -> Result<CompletionFixture, HostError> {
    for scope in [first, second] {
        scope.store_completion_inbox(91, "same-body").await?;
    }
    let first_identity = first
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 44 })
        .await?;
    let second_identity = second
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 44 })
        .await?;
    reserve(first, &first_identity).await?;
    reserve(second, &second_identity).await?;
    let (first_launch, second_launch) =
        begin_scoped_launches(first, second, &first_identity, &second_identity).await?;
    assert_eq!(
        first.record_runner_completed(44, 77, "v44").await?,
        Some(first_launch)
    );
    assert_eq!(
        second.record_runner_completed(44, 77, "v44").await?,
        Some(second_launch)
    );
    let first_inbox = first.pending_completion_inbox(0, 8).await?;
    let second_inbox = second.pending_completion_inbox(0, 8).await?;
    assert_eq!(first_inbox.len(), 1);
    assert_eq!(second_inbox.len(), 1);
    assert_eq!(first_inbox[0].message_id, second_inbox[0].message_id);
    assert_eq!(first_inbox[0].raw_body, second_inbox[0].raw_body);
    assert_eq!(first.due_completed_launches(0, 8).await?.len(), 1);
    assert_eq!(second.due_completed_launches(0, 8).await?.len(), 1);
    Ok(CompletionFixture {
        first_launch,
        second_launch,
        first_inbox,
        second_inbox,
    })
}

async fn begin_scoped_launches(
    first: &ConnectionJournal,
    second: &ConnectionJournal,
    first_identity: &AssignmentIdentity,
    second_identity: &AssignmentIdentity,
) -> Result<(i64, i64), HostError> {
    let (first_launch, first_fresh) = first.begin_assigned_launch(first_identity, "v44").await?;
    let (second_launch, second_fresh) =
        second.begin_assigned_launch(second_identity, "v44").await?;
    assert!(first_fresh && second_fresh);
    assert_ne!(first_launch, second_launch);
    Ok((first_launch, second_launch))
}

async fn assert_reopened_completion_rows(
    first: &ConnectionJournal,
    second: &ConnectionJournal,
    fixture: &CompletionFixture,
) -> Result<(), HostError> {
    assert_eq!(
        first.pending_completion_inbox(0, 8).await?,
        fixture.first_inbox
    );
    assert_eq!(
        second.pending_completion_inbox(0, 8).await?,
        fixture.second_inbox
    );
    let first_due = first.due_completed_launches(0, 8).await?;
    let second_due = second.due_completed_launches(0, 8).await?;
    assert_eq!(first_due.len(), 1);
    assert_eq!(second_due.len(), 1);
    assert_eq!(first_due[0].intent.id, fixture.first_launch);
    assert_eq!(second_due[0].intent.id, fixture.second_launch);
    assert_eq!(first_due[0].identity.runner_id, 77);
    assert_eq!(second_due[0].identity.runner_id, 77);
    Ok(())
}

async fn exercise_scoped_completion_cleanup(
    first: &ConnectionJournal,
    second: &ConnectionJournal,
    fixture: &CompletionFixture,
) -> Result<(), HostError> {
    let first_claim = first
        .claim_completion_cleanup_at(fixture.first_launch, 0, 10)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(first_claim.generation, 1);
    assert!(
        first
            .completion_cleanup_claim_current_at(fixture.first_launch, first_claim.generation, 1)
            .await?
    );
    assert!(
        second
            .claim_completion_cleanup_at(fixture.first_launch, 0, 10)
            .await?
            .is_none()
    );
    assert!(
        first
            .retry_completion_cleanup_at(fixture.first_launch, first_claim.generation, 1, 5)
            .await?
    );
    assert!(
        first
            .claim_completion_cleanup_at(fixture.first_launch, 4, 10)
            .await?
            .is_none()
    );
    let retry_claim = first
        .claim_completion_cleanup_at(fixture.first_launch, 5, 10)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(retry_claim.generation, 2);
    assert!(
        !first
            .completion_cleanup_claim_current_at(fixture.first_launch, first_claim.generation, 5)
            .await?
    );
    assert!(
        !first
            .record_completion_runner_absent_at(fixture.first_launch, first_claim.generation, 6)
            .await?
    );
    record_completion_cleanup(first, fixture.first_launch, retry_claim.generation).await?;
    settle_completion_inboxes(first, second, fixture).await
}

async fn record_completion_cleanup(
    first: &ConnectionJournal,
    intent_id: i64,
    generation: i64,
) -> Result<(), HostError> {
    assert_eq!(
        first
            .run_completion_cleanup_effect_at(intent_id, generation, 6, 5, || async {
                Ok::<_, HostError>(())
            })
            .await?,
        Some(())
    );
    assert!(
        first
            .bind_completion_containers_at(
                intent_id,
                generation,
                Some("runner-container"),
                Some("dind-container"),
                7,
            )
            .await?
    );
    assert!(
        first
            .record_completion_runner_absent_at(intent_id, generation, 8)
            .await?
    );
    assert!(
        first
            .record_completion_cleanup_at(intent_id, generation, 9)
            .await?
    );
    assert!(first.due_completed_launches(9, 8).await?.is_empty());
    Ok(())
}

async fn settle_completion_inboxes(
    first: &ConnectionJournal,
    second: &ConnectionJournal,
    fixture: &CompletionFixture,
) -> Result<(), HostError> {
    first
        .resolve_completion_inbox(&fixture.first_inbox[0])
        .await?;
    assert!(first.pending_completion_inbox(0, 8).await?.is_empty());
    assert_eq!(
        second.pending_completion_inbox(0, 8).await?,
        fixture.second_inbox
    );
    assert!(!first.is_drained().await?);
    assert!(!second.is_drained().await?);
    assert_eq!(second.due_completed_launches(9, 8).await?.len(), 1);
    Ok(())
}
