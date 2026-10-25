use crate::error::HostError;
use crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture;

async fn completed_native_launch()
-> Result<(NativeStartedIntakeFixture, crate::journal::CompletedLaunch), HostError> {
    let fixture = crate::journal::request_lifecycle::tests::native_started_intake_fixture().await?;
    let assignment = fixture
        .scope
        .assignment_identity(crate::journal::AssignmentKey::AssignedRequest {
            request_id: fixture.request_id,
        })
        .await?;
    fixture
        .scope
        .record_job_completed(
            &assignment,
            fixture.message_id,
            &fixture.source,
            Some("success"),
            Some(&fixture.runner),
            Some("opaque-started-job"),
        )
        .await?;
    let mut launches = fixture.scope.due_completed_launches(0, 1).await?;
    if launches.len() != 1 {
        return Err(HostError::Journal);
    }
    Ok((fixture, launches.remove(0)))
}

#[test]
fn reader_rejects_reservation_engine_mismatch() -> Result<(), String> {
    crate::launch::test_harness_stack::run_on_test_harness_stack(
        "reader_rejects_reservation_engine_mismatch",
        || {
            Box::pin(async {
                reader_rejects_reservation_engine_mismatch_on_fixture_stack()
                    .await
                    .map_err(|error| error.to_string())
            })
        },
    )
}

async fn reader_rejects_reservation_engine_mismatch_on_fixture_stack() -> Result<(), HostError> {
    let (fixture, launch) = completed_native_launch().await?;
    let connection = fixture.scope.journal.connection().await?;
    connection
        .execute(
            "UPDATE capacity_reservations SET engine_id = 'other-engine' WHERE id = ?1",
            [fixture.reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    drop(connection);

    assert!(matches!(
        fixture.scope.native_completion_reservation(&launch).await,
        Err(HostError::Identity)
    ));
    Ok(())
}

#[test]
fn reader_accepts_a_completed_launch_with_a_recovering_reservation() -> Result<(), String> {
    crate::launch::test_harness_stack::run_on_test_harness_stack(
        "reader_accepts_a_completed_launch_with_a_recovering_reservation",
        || {
            Box::pin(async {
                reader_accepts_a_completed_launch_with_a_recovering_reservation_on_fixture_stack()
                    .await
                    .map_err(|error| error.to_string())
            })
        },
    )
}

async fn reader_accepts_a_completed_launch_with_a_recovering_reservation_on_fixture_stack()
-> Result<(), HostError> {
    let (fixture, launch) = completed_native_launch().await?;
    fixture
        .scope
        .mark_recovering(fixture.reservation_id)
        .await?;

    let reservation = fixture
        .scope
        .native_completion_reservation(&launch)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(reservation.reservation_id(), fixture.reservation_id);
    assert_eq!(reservation.intent_id(), fixture.intent_id);
    assert_eq!(
        reservation.runner_container_id(),
        fixture.channel.runner_container_id()
    );
    assert_eq!(
        reservation.dind_container_id(),
        fixture.channel.dind_container_id()
    );
    Ok(())
}

#[test]
fn provisioning_rejects_engine_mismatch_with_reservation() -> Result<(), String> {
    crate::launch::test_harness_stack::run_on_test_harness_stack(
        "provisioning_rejects_engine_mismatch_with_reservation",
        || {
            Box::pin(async {
                provisioning_rejects_engine_mismatch_with_reservation_on_fixture_stack()
                    .await
                    .map_err(|error| error.to_string())
            })
        },
    )
}

async fn provisioning_rejects_engine_mismatch_with_reservation_on_fixture_stack()
-> Result<(), HostError> {
    let (fixture, launch) = completed_native_launch().await?;
    let connection = fixture.scope.journal.connection().await?;
    assert!(connection
        .execute(
            "UPDATE capacity_reservation_provisioning SET engine_id = 'other-engine' WHERE reservation_id = ?1",
            [fixture.reservation_id],
        )
        .await
        .is_err());
    drop(connection);

    assert!(
        fixture
            .scope
            .native_completion_reservation(&launch)
            .await?
            .is_some()
    );
    Ok(())
}
