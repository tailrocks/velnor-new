use crate::journal::{AssignmentKey, ReservationState};

#[test]
fn started_witness_does_not_block_cleanup_of_another_physical_lineage() -> Result<(), String> {
    crate::launch::test_harness_stack::run_on_test_harness_stack(
        "pre-jit-crossed-assignment",
        || Box::pin(started_witness_does_not_block_cleanup_of_another_physical_lineage_inner()),
    )
}

async fn started_witness_does_not_block_cleanup_of_another_physical_lineage_inner()
-> Result<(), String> {
    const OTHER_WITNESS: i64 = 9_101;
    let failure = Box::pin(crate::launch::runner_create_409_with_jobs(2)).await?;
    failure.set_probe_available_bytes(35 * 1024 * 1024 * 1024);
    let scope = failure.scope();
    let actual_request_id = failed_witness(scope, failure.reservation_id()).await?;
    let claim = scope
        .claim_pre_jit_cleanup(failure.reservation_id())
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The pre-JIT cleanup claim is absent.".to_owned())?;
    let connection = failure
        .host_journal()
        .connection()
        .await
        .map_err(|error| error.to_string())?;
    let other = prepare_started_other_lineage(&failure, OTHER_WITNESS).await?;
    record_crossed_started(scope, &connection, &other, actual_request_id).await?;

    let provider = crate::launch::IntakeCompositionProvider::new(
        failure.engine(),
        failure.images().source_sha(),
    );
    let released = crate::launch::bootstrap::remove_bootstrap_with_claim(
        failure.docker(),
        scope,
        failure.host_journal(),
        failure.engine(),
        failure.images(),
        &claim,
        &provider,
    )
    .await
    .map_err(|error| format!("The independent pre-JIT cleanup failed: {error}."))?;
    assert!(released);
    assert_eq!(
        scope
            .reservation(failure.assignment())
            .await
            .map_err(|error| error.to_string())?
            .unwrap()
            .state,
        ReservationState::Released,
    );
    assert_other_lineage_started(scope, other.reservation_id, actual_request_id).await?;
    assert_eq!(
        count_for_reservation(
            &connection,
            "SELECT COUNT(*) FROM physical_attempt_current WHERE reservation_id = ?1",
            failure.reservation_id(),
        )
        .await?,
        0,
        "The released physical lineage must have no current pointer."
    );
    assert_eq!(
        count_for_reservation(
            &connection,
            "SELECT COUNT(*) FROM connection_request_authorizations WHERE request_id = ?1 AND acquire_state = 'acquired' AND started_event_digest IS NULL AND completed_event_digest IS NULL",
            OTHER_WITNESS,
        )
        .await?,
        1,
        "The acquired witness for the other physical lineage must remain live."
    );
    failure.finish_and_assert_capacity_probe().await?;
    Ok(())
}

async fn failed_witness(
    scope: &crate::journal::ConnectionJournal,
    reservation_id: i64,
) -> Result<i64, String> {
    scope
        .physical_runner_reservations()
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|row| row.reservation_id == reservation_id)
        .and_then(|row| {
            row.acquired_request
                .and_then(|request| request.request_id())
        })
        .ok_or_else(|| "The failed lineage has no acquired witness.".to_owned())
}

struct OtherLineage {
    reservation_id: i64,
    runner: velnor_runner_github::RunnerReference,
}

async fn prepare_started_other_lineage(
    failure: &crate::launch::RunnerCreateFailure,
    witness_id: i64,
) -> Result<OtherLineage, String> {
    let connection = failure
        .host_journal()
        .connection()
        .await
        .map_err(|error| error.to_string())?;
    let filesystem_digest = disk_filesystem_digest(&connection, failure.reservation_id()).await?;
    let initialized = super::helpers::provision_acquired_request_with_engine_and_filesystem_digest(
        failure.scope(),
        witness_id,
        'b',
        'b',
        101,
        failure.engine().engine_id(),
        Some(filesystem_digest),
    )
    .await
    .map_err(|error| format!("The other physical lineage could not be prepared: {error}."))?;
    let runner = velnor_runner_github::RunnerReference {
        id: 702,
        name: super::helpers::runner_name('b'),
        runner_scale_set_id: 501,
    };
    super::helpers::finish_initialized_listener_delivery_for_engine(
        failure.scope(),
        failure.binding(),
        &initialized,
        &runner,
        witness_id,
        failure.engine().engine_id(),
    )
    .await
    .map_err(|error| format!("The other lineage listener delivery failed: {error}."))?;
    Ok(OtherLineage {
        reservation_id: initialized.reservation_id,
        runner,
    })
}

async fn disk_filesystem_digest(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<String, String> {
    let mut pools = connection
        .query("SELECT filesystem_instance_digest FROM capacity_reservation_provisioning_pools WHERE reservation_id = ?1 AND medium = 'disk' ORDER BY pool_id LIMIT 1", [reservation_id])
        .await
        .map_err(|error| error.to_string())?;
    pools
        .next()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The failed lineage has no disk-pool identity.".to_owned())?
        .get(0)
        .map_err(|error| error.to_string())
}

async fn record_crossed_started(
    scope: &crate::journal::ConnectionJournal,
    connection: &turso::Connection,
    other: &OtherLineage,
    actual_request_id: i64,
) -> Result<(), String> {
    let actual = scope
        .assignment_identity(AssignmentKey::AssignedRequest {
            request_id: actual_request_id,
        })
        .await
        .map_err(|error| error.to_string())?;
    let source = authorized_source_metadata(connection, actual_request_id).await?;
    scope
        .record_job_started(
            &actual,
            2_901,
            &source,
            &other.runner,
            Some("crossed-assignment-job"),
        )
        .await
        .map(|_| ())
        .map_err(|error| format!("The crossed Started event failed: {error}."))
}

async fn assert_other_lineage_started(
    scope: &crate::journal::ConnectionJournal,
    reservation_id: i64,
    actual_request_id: i64,
) -> Result<(), String> {
    let rows = scope
        .physical_runner_reservations()
        .await
        .map_err(|error| error.to_string())?;
    let row = rows
        .iter()
        .find(|row| row.reservation_id == reservation_id)
        .ok_or_else(|| "The Started physical lineage disappeared.".to_owned())?;
    assert_eq!(
        row.jit_state,
        crate::journal::PhysicalRunnerJitState::Started
    );
    assert_eq!(row.started_request_id, Some(actual_request_id));
    assert_eq!(row.reservation.state, ReservationState::Reserved);
    Ok(())
}

async fn count_for_reservation(
    connection: &turso::Connection,
    sql: &str,
    reservation_id: i64,
) -> Result<i64, String> {
    let mut rows = connection
        .query(sql, [reservation_id])
        .await
        .map_err(|error| error.to_string())?;
    rows.next()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The requested durable row count is absent.".to_owned())?
        .get(0)
        .map_err(|error| error.to_string())
}

async fn authorized_source_metadata(
    connection: &turso::Connection,
    request_id: i64,
) -> Result<velnor_runner_github::SourceMetadata, String> {
    let mut rows = connection
        .query(
            "SELECT source_json FROM connection_request_authorizations WHERE request_id = ?1",
            [request_id],
        )
        .await
        .map_err(|error| error.to_string())?;
    let encoded: String = rows
        .next()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The actual request source receipt is absent.".to_owned())?
        .get(0)
        .map_err(|error| error.to_string())?;
    let source: serde_json::Value =
        serde_json::from_str(&encoded).map_err(|error| error.to_string())?;
    Ok(velnor_runner_github::SourceMetadata {
        owner_name: source["owner_name"].as_str().map(str::to_owned),
        repository_name: source["repository_name"].as_str().map(str::to_owned),
        job_workflow_ref: source["job_workflow_ref"].as_str().map(str::to_owned),
        workflow_run_id: source["workflow_run_id"].as_i64(),
        event_name: source["event_name"].as_str().map(str::to_owned),
    })
}
