#[path = "capacity_pipeline/failures.rs"]
mod failures;

#[test]
fn terminal_runner_rejection_runs_owned_cleanup_and_capacity_probe() -> Result<(), String> {
    crate::launch::test_harness_stack::run_on_test_harness_stack(
        "pre-jit-terminal-runner-rejection",
        || Box::pin(terminal_runner_rejection_runs_owned_cleanup_and_capacity_probe_inner()),
    )
}

async fn terminal_runner_rejection_runs_owned_cleanup_and_capacity_probe_inner()
-> Result<(), String> {
    let failure = Box::pin(crate::launch::steps::runner_create_409()).await?;
    let provider = crate::launch::resource_probe::IntakeCompositionProvider::new(
        failure.engine(),
        failure.images().source_sha(),
    );
    let cleanup_result = crate::launch::bootstrap::remove_bootstrap_after_terminal_failure(
        failure.docker(),
        failure.scope(),
        failure.host_journal(),
        failure.engine(),
        failure.images(),
        failure.reservation_id(),
        &provider,
    )
    .await;
    let cleanup = match cleanup_result {
        Ok(cleanup) => cleanup,
        Err(error) => {
            let captured = failure.finish().await?;
            let requests = captured
                .requests
                .iter()
                .map(|request| {
                    format!(
                        "{} {} -> {:?}",
                        request.method, request.target, request.response_status
                    )
                })
                .collect::<Vec<_>>();
            return Err(format!(
                "Pre-JIT cleanup failed: {error}; fixture error: {:?}; requests: {requests:?}",
                captured.server_error
            ));
        }
    };
    assert!(cleanup);
    assert_eq!(
        failure
            .scope()
            .pre_jit_cleanup_settlement_outcome(failure.reservation_id())
            .await
            .map_err(|error| error.to_string())?,
        Some(crate::journal::PreJitCleanupSettlementResult::HeldAtCapacity),
        "The undersized valid Capacity sample must preserve the held reservation."
    );

    assert_capacity_probe_retired(&failure).await?;
    assert_eq!(
        failure.host_journal().occupied_reservation_count().await,
        Ok(1),
        "The pre-JIT cleanup path must keep the reservation charged."
    );
    let retry = crate::launch::bootstrap::remove_bootstrap_after_terminal_failure(
        failure.docker(),
        failure.scope(),
        failure.host_journal(),
        failure.engine(),
        failure.images(),
        failure.reservation_id(),
        &provider,
    )
    .await
    .map_err(|error| format!("The pre-JIT cleanup replay failed. Error: {error}."))?;
    assert!(
        !retry,
        "An active cleanup lease must prevent duplicate effects."
    );
    finish_and_assert_storage_volumes_removed(failure).await?;
    Ok(())
}

#[test]
fn available_capacity_releases_the_pre_jit_reservation() -> Result<(), String> {
    crate::launch::test_harness_stack::run_on_test_harness_stack(
        "pre-jit-available-capacity-release",
        || Box::pin(available_capacity_releases_the_pre_jit_reservation_inner()),
    )
}

async fn available_capacity_releases_the_pre_jit_reservation_inner() -> Result<(), String> {
    let failure = crate::launch::steps::runner_create_409().await?;
    failure.set_probe_available_bytes(35 * 1024 * 1024 * 1024);
    let provider = crate::launch::resource_probe::IntakeCompositionProvider::new(
        failure.engine(),
        failure.images().source_sha(),
    );
    let cleanup_result = crate::launch::bootstrap::remove_bootstrap_after_terminal_failure(
        failure.docker(),
        failure.scope(),
        failure.host_journal(),
        failure.engine(),
        failure.images(),
        failure.reservation_id(),
        &provider,
    )
    .await;
    let cleanup = match cleanup_result {
        Ok(cleanup) => cleanup,
        Err(error) => {
            let outcome = failure
                .scope()
                .pre_jit_cleanup_settlement_outcome(failure.reservation_id())
                .await
                .map_err(|lookup_error| lookup_error.to_string())?;
            return Err(format!(
                "The pre-JIT cleanup failed. Error: {error}. Recorded settlement: {outcome:?}."
            ));
        }
    };

    assert!(cleanup);
    assert_eq!(
        failure
            .scope()
            .pre_jit_cleanup_settlement_outcome(failure.reservation_id())
            .await
            .map_err(|error| error.to_string())?,
        Some(crate::journal::PreJitCleanupSettlementResult::CapacityAvailable),
        "The 35 GiB fresh Capacity sample must permit release."
    );
    assert_capacity_probe_retired(&failure).await?;
    assert_eq!(
        failure.host_journal().occupied_reservation_count().await,
        Ok(1),
        "The acquired logical request must remain live after physical release."
    );
    let reservation = failure
        .scope()
        .reservation(failure.assignment())
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The released pre-JIT reservation record is absent.".to_owned())?;
    assert_eq!(
        reservation.state,
        crate::journal::ReservationState::Released
    );
    assert!(
        failure
            .scope()
            .current_physical_attempt(failure.reservation_id())
            .await
            .map_err(|error| error.to_string())?
            .is_none()
    );
    assert_acquired_demand_stays_live(&failure).await?;
    failure.finish_and_assert_capacity_probe().await?;
    Ok(())
}

#[test]
fn rejected_release_rolls_back_outcome_charge_and_current_pointer() -> Result<(), String> {
    crate::launch::test_harness_stack::run_on_test_harness_stack(
        "pre-jit-rejected-release-rollback",
        || Box::pin(rejected_release_rolls_back_outcome_charge_and_current_pointer_inner()),
    )
}

async fn rejected_release_rolls_back_outcome_charge_and_current_pointer_inner() -> Result<(), String>
{
    let failure = crate::launch::steps::runner_create_409().await?;
    failure.set_probe_available_bytes(35 * 1024 * 1024 * 1024);
    let connection = failure
        .host_journal()
        .connection()
        .await
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            &format!(
                "CREATE TRIGGER reject_pre_jit_release BEFORE UPDATE OF state ON capacity_reservations WHEN OLD.id = {} AND NEW.state = 'released' BEGIN SELECT RAISE(ABORT, 'test release rejection'); END",
                failure.reservation_id()
            ),
            (),
        )
        .await
        .map_err(|error| error.to_string())?;

    let provider = crate::launch::resource_probe::IntakeCompositionProvider::new(
        failure.engine(),
        failure.images().source_sha(),
    );
    let result = crate::launch::bootstrap::remove_bootstrap_after_terminal_failure(
        failure.docker(),
        failure.scope(),
        failure.host_journal(),
        failure.engine(),
        failure.images(),
        failure.reservation_id(),
        &provider,
    )
    .await;
    if result.is_ok() {
        return Err("The injected release failure did not stop cleanup.".to_owned());
    }
    assert_eq!(
        failure
            .scope()
            .pre_jit_cleanup_settlement_outcome(failure.reservation_id())
            .await
            .map_err(|error| error.to_string())?,
        Some(crate::journal::PreJitCleanupSettlementResult::CapacityAvailable)
    );
    let reservation = failure
        .scope()
        .reservation(failure.assignment())
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The uncertain reservation record is absent.".to_owned())?;
    if reservation.state != crate::journal::ReservationState::Uncertain
        || failure
            .scope()
            .current_physical_attempt(failure.reservation_id())
            .await
            .map_err(|error| error.to_string())?
            .is_none()
    {
        return Err("A failed release changed the charge or current attempt.".to_owned());
    }
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM physical_attempt_outcomes WHERE reservation_id = ?1",
            [failure.reservation_id()],
        )
        .await
        .map_err(|error| error.to_string())?;
    let row = rows
        .next()
        .await
        .map_err(|error| error.to_string())
        .and_then(|row| row.ok_or_else(|| "The outcome count is absent.".to_owned()))?;
    let outcomes: i64 = row.get(0).map_err(|error| error.to_string())?;
    if outcomes != 0 {
        return Err("The failed release left a terminal physical outcome.".to_owned());
    }
    failure.finish_and_assert_capacity_probe().await?;
    Ok(())
}

async fn assert_acquired_demand_stays_live(
    failure: &crate::launch::steps::RunnerCreateFailure,
) -> Result<(), String> {
    let connection = failure
        .host_journal()
        .connection()
        .await
        .map_err(|error| error.to_string())?;
    let mut rows = connection
        .query(
            "SELECT authorization.acquire_state, authorization.started_event_digest, authorization.completed_event_digest FROM physical_attempts AS attempt JOIN connection_request_authorizations AS authorization ON authorization.connection_id = attempt.connection_id AND authorization.scale_set_id = attempt.scale_set_id AND authorization.request_id = attempt.witness_request_id WHERE attempt.reservation_id = ?1",
            [failure.reservation_id()],
        )
        .await
        .map_err(|error| error.to_string())?;
    let row = rows
        .next()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The historical acquired request record is absent.".to_owned())?;
    let acquire_state: String = row.get(0).map_err(|error| error.to_string())?;
    let started_digest: Option<String> = row.get(1).map_err(|error| error.to_string())?;
    let completed_digest: Option<String> = row.get(2).map_err(|error| error.to_string())?;
    if rows
        .next()
        .await
        .map_err(|error| error.to_string())?
        .is_some()
        || acquire_state != "acquired"
        || started_digest.is_some()
        || completed_digest.is_some()
    {
        return Err("Physical cleanup changed or duplicated logical request state.".to_owned());
    }
    Ok(())
}

async fn assert_capacity_probe_retired(
    failure: &crate::launch::steps::RunnerCreateFailure,
) -> Result<(), String> {
    let active = failure
        .host_journal()
        .active_probe()
        .await
        .map_err(|error| error.to_string())?;
    if active.is_some() {
        return Err("The Capacity probe stayed active after observation.".to_owned());
    }
    Ok(())
}

fn finish_and_assert_storage_volumes_removed(
    failure: crate::launch::steps::RunnerCreateFailure,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>>>> {
    Box::pin(async move {
        let expected = failure.journaled_volume_expectations().await?;
        let connection_id = failure.binding().connection_id.as_str().to_owned();
        let reservation_id = failure.reservation_id();
        let captured = failure.finish().await?;
        if let Some(error) = captured.server_error {
            return Err(format!("The Docker fixture failed. Error: {error}."));
        }
        crate::launch::steps::RunnerCreateFailure::assert_journaled_volume_create_requests(
            &expected,
            &connection_id,
            reservation_id,
            &captured.requests,
        )?;
        let mut created = captured
            .requests
            .iter()
            .filter(|request| {
                request.method == "POST" && request.target.starts_with("/volumes/create")
            })
            .map(|request| {
                let body: serde_json::Value =
                    serde_json::from_slice(&request.body).map_err(|error| error.to_string())?;
                body["Name"]
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| "A journaled storage create has no volume name.".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?;
        created.sort_unstable();
        created.dedup();
        if created.len() != 5 {
            return Err(format!(
                "The acquired bootstrap must create each of its five journaled volumes once; found {}.",
                created.len()
            ));
        }
        let mut removed = captured
        .requests
        .iter()
        .filter(|request| request.method == "DELETE" && request.target.starts_with("/volumes/"))
        .map(|request| {
            if request.response_status != Some(204) {
                return Err(format!(
                    "The journaled volume delete did not succeed: {} returned {:?}.",
                    request.target, request.response_status
                ));
            }
            let name = request
                .target
                .strip_prefix("/volumes/")
                .ok_or_else(|| "The volume delete path is invalid.".to_owned())?;
            let index = captured
                .requests
                .iter()
                .position(|candidate| std::ptr::eq(candidate, request))
                .ok_or_else(|| "The volume delete request disappeared.".to_owned())?;
            if !captured.requests[index + 1..].iter().any(|candidate| {
                candidate.method == "GET"
                    && candidate.target == format!("/volumes/{name}")
                    && candidate.response_status == Some(404)
            }) {
                return Err(format!(
                    "The journaled volume {name} was not inspected after deletion to confirm that it was absent."
                ));
            }
            Ok(name.to_owned())
        })
        .collect::<Result<Vec<_>, _>>()?;
        removed.sort_unstable();
        if removed != created {
            return Err(format!(
                "Cleanup did not delete exactly the five journaled storage volumes: {removed:?}."
            ));
        }
        Ok(())
    })
}
