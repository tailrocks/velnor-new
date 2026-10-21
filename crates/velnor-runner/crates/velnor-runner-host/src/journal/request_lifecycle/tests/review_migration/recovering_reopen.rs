use crate::error::HostError;
use crate::journal::{AssignmentIdentity, ConnectionJournal, Journal, ReservationState};

#[test]
fn completed_recovering_reservation_is_read_after_reopen() -> Result<(), String> {
    crate::launch::test_harness_stack::run_on_test_harness_stack(
        "completed_recovering_reservation_is_read_after_reopen",
        || {
            Box::pin(async {
                completed_recovering_reservation_is_read_after_reopen_on_fixture_stack()
                    .await
                    .map_err(|error| error.to_string())
            })
        },
    )
}

async fn completed_recovering_reservation_is_read_after_reopen_on_fixture_stack()
-> Result<(), HostError> {
    let (_scratch, journal, scope, launch, reservation_id, intent_id, runner, dind, _) =
        completed_recovering_launch(false).await?;
    let reservation = scope
        .native_completion_reservation(&launch)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(reservation.reservation_id(), reservation_id);
    assert_eq!(reservation.intent_id(), intent_id);
    assert_eq!(reservation.runner_container_id(), runner);
    assert_eq!(reservation.dind_container_id(), dind);
    drop(scope);
    drop(journal);
    Ok(())
}

#[test]
fn incomplete_cleanup_proofs_keep_recovering_charge_after_reopen() -> Result<(), String> {
    crate::launch::test_harness_stack::run_on_test_harness_stack(
        "incomplete_cleanup_proofs_keep_recovering_charge_after_reopen",
        || {
            Box::pin(async {
                incomplete_cleanup_proofs_keep_recovering_charge_after_reopen_on_fixture_stack()
                    .await
                    .map_err(|error| error.to_string())
            })
        },
    )
}

async fn incomplete_cleanup_proofs_keep_recovering_charge_after_reopen_on_fixture_stack()
-> Result<(), HostError> {
    let (_scratch, journal, scope, launch, _reservation_id, _, _, _, assignment) =
        completed_recovering_launch(true).await?;
    let reservation = scope
        .native_completion_reservation(&launch)
        .await?
        .ok_or(HostError::Journal)?;
    assert!(reservation.remote_cleanup_proven());
    assert!(!reservation.storage_cleanup_proven());
    assert!(!reservation.network_cleanup_proven());
    assert!(!reservation.local_cleanup_proven());
    let row = scope
        .reservation(&assignment)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(row.state, ReservationState::Recovering);
    drop(scope);
    drop(journal);
    Ok(())
}

async fn completed_recovering_launch(
    record_partial_cleanup: bool,
) -> Result<
    (
        crate::journal::request_lifecycle::tests::helpers::Scratch,
        Journal,
        ConnectionJournal,
        crate::journal::CompletedLaunch,
        i64,
        i64,
        String,
        String,
        AssignmentIdentity,
    ),
    HostError,
> {
    let fixture = crate::journal::request_lifecycle::tests::native_started_intake_fixture().await?;
    fixture.record_started().await?;
    fixture
        .scope
        .record_job_completed(
            &fixture.assignment,
            fixture.message_id + 1,
            &fixture.source,
            Some("success"),
            Some(&fixture.runner),
            Some("opaque-started-job"),
        )
        .await?;
    fixture
        .scope
        .mark_recovering(fixture.reservation_id)
        .await?;
    if record_partial_cleanup {
        fixture
            .scope
            .record_remote_cleanup(fixture.reservation_id)
            .await?;
    }
    let mut launches = fixture.scope.due_completed_launches(0, 1).await?;
    if launches.len() != 1 {
        return Err(HostError::Journal);
    }
    let database_path = fixture.database_path.clone();
    let reservation_id = fixture.reservation_id;
    let intent_id = fixture.intent_id;
    let runner = fixture.channel.runner_container_id().to_owned();
    let dind = fixture.channel.dind_container_id().to_owned();
    let assignment = fixture.reservation_assignment.clone();
    let scratch = fixture._scratch;
    drop(fixture.scope);
    drop(fixture.journal);

    let journal = Journal::open(&database_path).await?;
    let binding =
        crate::journal::request_lifecycle::tests::helpers::binding("native-started-intake", 101)?;
    let scope = journal.connection_scope(&binding).await?;
    let mut reopened = scope.due_completed_launches(0, 1).await?;
    if reopened.len() != 1 {
        return Err(HostError::Journal);
    }
    launches.clear();
    Ok((
        scratch,
        journal,
        scope,
        reopened.remove(0),
        reservation_id,
        intent_id,
        runner,
        dind,
        assignment,
    ))
}
