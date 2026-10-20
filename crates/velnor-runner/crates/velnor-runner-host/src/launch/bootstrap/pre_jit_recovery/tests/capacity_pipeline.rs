#[tokio::test]
async fn terminal_runner_rejection_runs_owned_cleanup_and_capacity_probe() -> Result<(), String> {
    let failure = crate::launch::steps::runner_create_409().await?;
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
    failure.finish_and_assert_capacity_probe().await?;
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
    let reservation = failure
        .scope()
        .reservation(failure.assignment())
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The pre-JIT reservation disappeared.".to_owned())?;
    if reservation.state == crate::journal::ReservationState::Released {
        return Err("The pre-JIT reservation was released after local settlement.".to_owned());
    }
    Ok(())
}
