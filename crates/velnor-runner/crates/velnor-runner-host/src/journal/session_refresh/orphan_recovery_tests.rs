use std::path::Path;

use crate::daemon_lock::{DaemonLock, EngineLineageGuard, canonical_journal_path};
use crate::error::HostError;
use crate::journal::{Journal, ReservationState};

async fn recover_startup(journal: &Journal, path: &Path) -> Result<(), HostError> {
    let canonical_path = canonical_journal_path(path)?;
    let state_dir = path.parent().ok_or(HostError::Path)?;
    let daemon_lock_path = state_dir.join("daemon.lock");
    let daemon_lock = DaemonLock::try_acquire(&daemon_lock_path)?;
    let engine_id = journal.engine_id().await?;
    let instance_id = journal.instance_id().await?;
    let guard = EngineLineageGuard::acquire_at_for_tests(&engine_id, state_dir)?;
    guard.verify_lineage(&canonical_path, &instance_id, journal.revision().await?)?;
    journal.attach_lineage_guard(guard.clone())?;
    journal.bootstrap_header_authority().await?;
    let authority = crate::launch::SessionRefreshStartupAuthority::test_only(
        &daemon_lock,
        &daemon_lock_path,
        &canonical_path,
        &instance_id,
        &engine_id,
        journal.revision().await?,
        &guard,
    )?;
    journal.recover_interrupted_host_work(authority).await
}

async fn proof_state(journal: &Journal) -> Result<(String, String, Option<String>), HostError> {
    let connection = journal.connection().await?;
    let mut rows = connection
        .query(
            "SELECT proof_state, challenge_state, deny_code FROM connection_execution_proofs ORDER BY reservation_id LIMIT 1",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    Ok((
        row.get(0).map_err(|_| HostError::Journal)?,
        row.get(1).map_err(|_| HostError::Journal)?,
        row.get(2).map_err(|_| HostError::Journal)?,
    ))
}

#[tokio::test]
async fn published_interrupted_runner_is_marked_atomically_and_survives_reopen()
-> Result<(), HostError> {
    let fixture = crate::journal::request_lifecycle::tests::native_started_intake_fixture().await?;
    let connection = fixture.journal.connection().await?;
    connection
        .execute(
            "CREATE TRIGGER fail_orphan_recovery BEFORE UPDATE OF state ON capacity_reservations WHEN NEW.state = 'recovering' BEGIN SELECT RAISE(ABORT, 'orphan recovery rollback'); END",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;

    let crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture {
        _scratch: scratch,
        journal,
        database_path,
        scope,
        ..
    } = fixture;
    assert!(recover_startup(&journal, &database_path).await.is_err());
    assert_eq!(
        proof_state(&journal).await?,
        ("pending".to_owned(), "published".to_owned(), None)
    );
    connection
        .execute("DROP TRIGGER fail_orphan_recovery", ())
        .await
        .map_err(|_| HostError::Journal)?;
    drop(connection);
    drop(scope);
    drop(journal);
    let journal = Journal::open(&database_path).await?;
    let binding =
        crate::journal::request_lifecycle::tests::helpers::binding("native-started-intake", 101)?;
    let scope = journal.connection_scope(&binding).await?;
    recover_startup(&journal, &database_path).await?;
    assert_eq!(
        proof_state(&journal).await?,
        (
            "denied".to_owned(),
            "abandoned".to_owned(),
            Some("interrupted_after_restart".to_owned())
        )
    );
    let reservations = scope.physical_runner_reservations().await?;
    assert_eq!(reservations.len(), 1);
    assert_eq!(
        reservations[0].reservation.state,
        ReservationState::Recovering
    );

    drop(scope);
    drop(journal);
    let reopened = Journal::open(&database_path).await?;
    let binding =
        crate::journal::request_lifecycle::tests::helpers::binding("native-started-intake", 101)?;
    let scope = reopened.connection_scope(&binding).await?;
    assert_eq!(
        proof_state(&reopened).await?,
        (
            "denied".to_owned(),
            "abandoned".to_owned(),
            Some("interrupted_after_restart".to_owned())
        )
    );
    assert_eq!(
        scope.physical_runner_reservations().await?[0]
            .reservation
            .state,
        ReservationState::Recovering
    );
    drop(reopened);
    drop(scratch);
    Ok(())
}

#[tokio::test]
async fn uncertain_attempt_is_not_marked_for_orphan_recovery() -> Result<(), HostError> {
    let uncertain =
        crate::journal::request_lifecycle::tests::native_pre_challenge_fixture().await?;
    let crate::journal::request_lifecycle::tests::NativePreChallengeFixture {
        _journal: journal,
        ..
    } = &uncertain;
    let claim = uncertain
        .scope
        .claim_execution_challenge(uncertain.initialized.reservation_id, &uncertain.channel)
        .await?
        .ok_or(HostError::Journal)?;
    uncertain
        .scope
        .mark_execution_challenge_uncertain(&claim)
        .await?;
    recover_startup(journal, &uncertain.database_path).await?;
    assert_eq!(
        proof_state(journal).await?,
        (
            "denied".to_owned(),
            "uncertain".to_owned(),
            Some("interrupted_after_restart".to_owned())
        )
    );
    assert_eq!(
        uncertain.scope.physical_runner_reservations().await?[0]
            .reservation
            .state,
        ReservationState::Reserved
    );
    Ok(())
}

#[tokio::test]
async fn started_attempt_is_not_marked_for_orphan_recovery() -> Result<(), HostError> {
    let started = crate::journal::request_lifecycle::tests::native_started_intake_fixture().await?;
    started.record_started().await?;
    recover_startup(&started.journal, &started.database_path).await?;
    assert_eq!(
        started.scope.physical_runner_reservations().await?[0]
            .reservation
            .state,
        ReservationState::Reserved
    );
    let connection = started.journal.connection().await?;
    let mut rows = connection
        .query(
            "SELECT state FROM connection_runner_jits WHERE intent_id = ?1",
            [started.intent_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    assert_eq!(
        row.get::<String>(0).map_err(|_| HostError::Journal)?,
        "started"
    );
    Ok(())
}
