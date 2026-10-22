use super::*;

#[test]
fn invalid_channel_record_sends_no_ack_and_keeps_acquired_charge() -> Result<(), String> {
    const STACK_BYTES: usize = 8 * 1024 * 1024;
    std::thread::Builder::new()
        .name("invalid-channel-record-test".to_owned())
        .stack_size(STACK_BYTES)
        .spawn(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| error.to_string())?;
            runtime.block_on(check_invalid_channel_records())
        })
        .map_err(|error| error.to_string())?
        .join()
        .map_err(|_| "The invalid-channel test thread panicked.".to_owned())?
}

async fn check_invalid_channel_records() -> Result<(), String> {
    Box::pin(check_invalid_channel_record(false)).await?;
    Box::pin(check_invalid_channel_record(true)).await
}

async fn check_invalid_channel_record(tamper_attachment: bool) -> Result<(), String> {
    let mut setup = bootstrap_start::open_composition().await?;
    if tamper_attachment {
        setup.api.tamper_next_channel_attachment_inode();
    } else {
        setup.api.tamper_next_channel_operation_id();
    }
    let acquired = setup
        .outcome
        .take()
        .ok_or_else(|| "The acquired request is absent.".to_owned())?;
    let provider = crate::launch::resource_probe::IntakeCompositionProvider::new(
        &setup.engine,
        setup.images.source_sha(),
    );
    let bound = Box::pin(bind_acquired(&setup, &provider, acquired)).await?;
    let failure = Box::pin(create_and_measure_channel(&setup, bound))
        .await
        .err()
        .ok_or_else(|| "The invalid channel record was accepted.".to_owned())?;
    if !failure.contains("channel measurement failed") {
        return Err(format!(
            "The channel failed before response validation. Error: {failure}."
        ));
    }
    let request_id = setup
        .fixture
        .request
        .request_id()
        .ok_or_else(|| "The acquired request has no native request ID.".to_owned())?;
    let reservations = setup
        .fixture
        .scope
        .physical_runner_reservations()
        .await
        .map_err(|error| error.to_string())?;
    let row = reservations
        .iter()
        .find(|row| {
            row.acquired_request
                .as_ref()
                .is_some_and(|item| item.request_id() == Some(request_id))
        })
        .ok_or_else(|| "The acquired reservation charge is absent.".to_owned())?;
    assert_eq!(
        row.reservation.state,
        crate::journal::ReservationState::Reserved
    );
    assert_eq!(row.jit_state, crate::journal::PhysicalRunnerJitState::Bound);
    assert_eq!(row.started_request_id, None);
    assert_eq!(row.acquired_request.as_ref(), Some(&setup.fixture.request));
    assert_eq!(
        setup.fixture.journal.occupied_reservation_count().await,
        Ok(1)
    );
    let publication = setup
        .fixture
        .scope
        .native_publication_snapshot(row.reservation_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The durable reservation publication state is absent.".to_owned())?;
    assert_eq!(publication.jit_state, "bound");
    assert_eq!(publication.publication_state, "not_requested");
    assert_eq!(publication.delivery_state, "not_requested");
    assert_eq!(publication.challenge_state, None);

    let captured = setup.api.finish().await?;
    let server_error = captured
        .server_error
        .ok_or_else(|| "The Docker exec server received a success acknowledgment.".to_owned())?;
    if !server_error.contains("without a complete line") {
        return Err(format!(
            "The Docker exec server failed unexpectedly. Error: {server_error}."
        ));
    }
    assert_eq!(captured.jit_input, [] as [u8; 0]);
    Ok(())
}
