use super::*;

#[tokio::test]
async fn session_drain_read_is_scoped_and_delete_rechecks_work() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let (journal, scope) = open_scope(&scratch.file(), "essential", 101, 2).await?;
    let prepared = scope
        .prepare_session_create("ffffffffffffffffffffffffffffffff")
        .await?;
    let claim = scope
        .claim_session_create(prepared.local_nonce())
        .await?
        .ok_or(HostError::Journal)?;
    let bound = scope
        .record_created_session(&claim, "remote-session", 1)
        .await?;
    let intent_id = bound.intent_id();
    let allocation = bound.allocation().clone();

    assert!(scope.session_work_drained(intent_id).await?);
    let other_work = scope.begin("runner", "unrelated-work").await?;
    assert!(!scope.session_work_drained(intent_id).await?);
    scope.finish(other_work, Outcome::Done).await?;
    scope.record_cleanup(other_work).await?;
    assert!(scope.session_work_drained(intent_id).await?);

    let sibling = journal
        .connection_scope(&binding("chainargos", 202)?)
        .await?;
    sibling
        .bind_scale_set(10_202, "integration", SetOwnership::Adopted)
        .await?;
    let _sibling_work = sibling.begin("runner", "sibling-work").await?;
    assert!(scope.session_work_drained(intent_id).await?);

    let zero = scope.begin_zero_header_poll(intent_id, &allocation).await?;
    scope
        .settle_session_poll(intent_id, CompletedHeaderPoll::test_only(zero))
        .await?;
    let later_work = scope.begin("runner", "later-work").await?;
    assert!(!scope.session_work_drained(intent_id).await?);
    assert!(
        scope
            .begin_session_delete(intent_id, &allocation)
            .await
            .is_err()
    );
    scope.finish(later_work, Outcome::Done).await?;
    scope.record_cleanup(later_work).await?;
    assert!(scope.session_work_drained(intent_id).await?);
    let final_zero = scope.begin_zero_header_poll(intent_id, &allocation).await?;
    scope
        .settle_session_poll(intent_id, CompletedHeaderPoll::test_only(final_zero))
        .await?;
    let delete = scope.begin_session_delete(intent_id, &allocation).await?;
    scope
        .retire_session_after_delete(intent_id, DeletedSession204::test_only(delete))
        .await?;
    assert_eq!(journal.charged_header_capacity().await?, 0);
    Ok(())
}

#[tokio::test]
async fn session_drain_stays_closed_while_legacy_work_is_ambiguous() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let (journal, scope) = open_scope(&scratch.file(), "essential", 101, 1).await?;
    let prepared = scope
        .prepare_session_create("99999999999999999999999999999999")
        .await?;
    let claim = scope
        .claim_session_create(prepared.local_nonce())
        .await?
        .ok_or(HostError::Journal)?;
    let bound = scope
        .record_created_session(&claim, "remote-session", 1)
        .await?;
    assert!(scope.session_work_drained(bound.intent_id()).await?);

    journal.begin("legacy-runner", "unscoped-work").await?;
    assert!(!scope.session_work_drained(bound.intent_id()).await?);
    let zero = scope
        .begin_zero_header_poll(bound.intent_id(), bound.allocation())
        .await?;
    scope
        .settle_session_poll(bound.intent_id(), CompletedHeaderPoll::test_only(zero))
        .await?;
    assert!(
        scope
            .begin_session_delete(bound.intent_id(), bound.allocation())
            .await
            .is_err()
    );
    assert_eq!(journal.charged_header_capacity().await?, 1);
    Ok(())
}

#[tokio::test]
async fn interrupted_poll_stays_charged_and_cannot_be_retired() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let (journal, scope) = open_scope(&path, "essential", 101, 1).await?;
    let prepared = scope
        .prepare_session_create("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee")
        .await?;
    let claim = scope
        .claim_session_create(prepared.local_nonce())
        .await?
        .ok_or(HostError::Journal)?;
    let bound = scope
        .record_created_session(&claim, "remote-session", 1)
        .await?;
    let allocation = bound.allocation().clone();
    let intent_id = bound.intent_id();
    let _in_flight = scope.begin_session_poll(intent_id, &allocation).await?;
    drop(scope);
    drop(journal);

    let (journal, scope) = open_scope(&path, "essential", 101, 1).await?;
    assert_eq!(journal.charged_header_capacity().await?, 1);
    assert!(
        scope
            .begin_session_poll(intent_id, &allocation)
            .await
            .is_err()
    );
    assert!(
        scope
            .begin_session_delete(intent_id, &allocation)
            .await
            .is_err()
    );
    assert!(!scope.is_drained().await?);
    assert!(!journal.is_drained().await?);
    Ok(())
}

#[tokio::test]
async fn altered_session_index_predicate_fails_closed_on_reopen() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    let connection = journal.connection().await?;
    connection
        .execute("DROP INDEX connection_session_create_active", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "CREATE UNIQUE INDEX connection_session_create_active ON connection_session_creates(connection_id, scale_set_id) WHERE state IN ('prepared', 'requested', 'uncertain')",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    drop(connection);
    drop(journal);

    assert!(Journal::open(&path).await.is_err());
    Ok(())
}

#[tokio::test]
async fn altered_session_table_ddl_fails_closed_on_reopen() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    let connection = journal.connection().await?;
    connection
        .execute(
            "ALTER TABLE connection_session_creates ADD COLUMN unexpected TEXT",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    drop(connection);
    drop(journal);

    assert!(Journal::open(&path).await.is_err());
    Ok(())
}
