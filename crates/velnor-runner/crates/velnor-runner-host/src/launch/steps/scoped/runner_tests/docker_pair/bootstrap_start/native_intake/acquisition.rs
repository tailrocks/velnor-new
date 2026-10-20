use super::*;

pub(in crate::launch::steps::scoped::runner::tests::docker_pair) async fn authorize_actual_started_request(
    fixture: &Fixture,
    request_id: i64,
) -> Result<(), String> {
    let request = fixture
        .scope
        .assignment_identity(crate::journal::AssignmentKey::AssignedRequest { request_id })
        .await
        .map_err(|error| error.to_string())?;
    let binding = fixture
        .scope
        .source_authorization_binding()
        .await
        .map_err(|error| error.to_string())?;
    let receipt = crate::launch::steps::source::test_receipt(
        &binding,
        request_id,
        128,
        super::super::super::super::support::verified_source(),
        crate::config::SourcePolicyMode::Repository,
    );
    if fixture
        .scope
        .record_source_authorization(&request, &receipt)
        .await
        .map_err(|error| error.to_string())?
        != crate::journal::SourceAuthorizationResult::Recorded
    {
        return Err("The actual Started request source was not recorded.".to_owned());
    }
    if !fixture
        .scope
        .claim_request_acquire(&request)
        .await
        .map_err(|error| error.to_string())?
    {
        return Err("The actual Started request acquire was not recorded.".to_owned());
    }
    fixture
        .scope
        .record_request_acquire(
            &request,
            crate::journal::AssignedAcquireResult::Returned(vec![request_id]),
        )
        .await
        .map_err(|error| error.to_string())
}

pub(in crate::launch::steps::scoped::runner::tests::docker_pair) async fn open_composition_with_poll(
    poll: &velnor_runner_github::Poll,
    backlog_rows: usize,
) -> Result<super::CompositionSetup, String> {
    let fixture = Fixture::open_with_engine("native-intake-composition", ENGINE_ID).await?;
    configure_capacity(&fixture).await?;
    let poll_session = Some(create_poll_session(&fixture).await?);
    let permit = StartPermit::test_fixture_with_engine_and_available_memory(
        &"a".repeat(32),
        ENGINE_ID,
        MEMORY_BYTES,
    );
    let images = super::super::super::super::support::test_images_for_engine(
        ENGINE_ID,
        permit.docker_root_digest(),
    );
    let engine = selected_engine(&images)?;
    let state = BootstrapAdmissionState::new(&fixture.binding, 1)?;
    let drive = bootstrap_drive(&fixture, &state.path);
    if backlog_rows > 0 {
        seed_native_inbox(&fixture.scope, backlog_rows).await?;
    }
    let poll_session_ref = poll_session
        .as_ref()
        .ok_or_else(|| "The native intake poll session is absent.".to_owned())?;
    let outcome =
        acquire_through_positive_poll(&fixture, poll_session_ref, &drive, permit, &engine, poll)
            .await?;
    let guest_image =
        crate::worker::guest_image::test_support::TestGuestImage::for_connection_and_images(
            fixture.binding.clone(),
            images.clone(),
        )
        .map_err(|error| error.to_string())?;
    let (initializer_image, initializer) = initializer_for(&outcome, &fixture, &images).await?;
    let api = PairDockerApi::open_with_initializer(images.clone(), initializer_image, initializer)?;
    Ok(super::CompositionSetup {
        fixture,
        api,
        outcome: Some(outcome),
        engine,
        images,
        guest_image,
        poll_session,
        _state: state,
    })
}

async fn acquire_through_positive_poll(
    fixture: &Fixture,
    poll_session: &PollSession,
    drive: &crate::launch::Drive,
    permit: crate::launch::StartPermit,
    engine: &crate::launch::SelectedEngineContext,
    polled: &velnor_runner_github::Poll,
) -> Result<crate::launch::steps::ScopedProvisioningOutcome, String> {
    let mut link = crate::listen::inert_link_for_offline_test();
    let mut session =
        velnor_runner_github::create_session(&mut SessionResponse, SET_ID, "owner", "admin-token")
            .map_err(|error| error.to_string())?;
    let mut permit = Some(permit);
    let acquire_calls = poll_session.acquire_calls.clone();
    let acknowledgement_calls = poll_session.acknowledgement_calls.clone();
    let mut lane = FakeLane::new([Ok(exchange(200, r#"{"count":1,"value":[41]}"#))]);
    let outcome = crate::launch::test_positive_poll_once_with_response(
        &mut link,
        SET_ID,
        &mut session,
        &fixture.scope,
        poll_session.intent_id,
        &poll_session.allocation,
        0,
        polled.clone(),
        "https://queue.example.invalid/messages".to_owned(),
        Some("queue-token".to_owned()),
        async |_link, _session, _path, _queue, _polled| {
            acknowledgement_calls.set(acknowledgement_calls.get() + 1);
            Ok(())
        },
        async |_link, poll, _queue, _path, _queue_token, _admin_token| {
            acquire_calls.set(acquire_calls.get() + 1);
            let permit = permit
                .take()
                .ok_or(crate::scale_set::EnsureError::Uncertain)?;
            let acquired =
                crate::launch::steps::acquire_scoped(&mut lane, drive, poll, permit, engine)
                    .await?;
            match acquired {
                crate::launch::steps::ScopedProvisioningOutcome::Acquired(acquired) => {
                    Ok(crate::launch::PositiveAdmission::Acquired(acquired))
                }
                crate::launch::steps::ScopedProvisioningOutcome::NoStart
                | crate::launch::steps::ScopedProvisioningOutcome::KnownNoAcquire(_) => {
                    Ok(crate::launch::PositiveAdmission::NoAcquire)
                }
                crate::launch::steps::ScopedProvisioningOutcome::Recovered(_) => {
                    Err(crate::scale_set::EnsureError::Uncertain)
                }
            }
        },
    )
    .await
    .map_err(|error| format!("The shared positive-poll admission failed: {error}"))?;
    if lane.paths.len() != 1 || !lane.paths[0].ends_with("/acquirejobs") {
        return Err(format!("Unexpected native Acquire paths: {:?}", lane.paths));
    }
    if outcome.acknowledgement != crate::launch::Acknowledgement::Acknowledged
        || outcome.settlement != crate::launch::Settlement::Settled
    {
        return Err(
            "The positive poll did not settle and acknowledge the acquired request.".to_owned(),
        );
    }
    let cleanup_wake_expected = matches!(
        polled,
        velnor_runner_github::Poll::Batch(batch)
            if batch.jobs.iter().any(|job| matches!(
                job.kind,
                velnor_runner_github::InnerKind::Completed
            ))
    );
    if outcome.wake_cleanup != cleanup_wake_expected || !outcome.started_request_ids.is_empty() {
        return Err(
            "The poll lifecycle outcome did not match its messages before admission.".to_owned(),
        );
    }
    match outcome.admission {
        Some(crate::launch::PositiveAdmission::Acquired(acquired)) => Ok(
            crate::launch::steps::ScopedProvisioningOutcome::Acquired(acquired),
        ),
        _ => Err("The positive poll did not retain its acquired reservation.".to_owned()),
    }
}

async fn create_poll_session(fixture: &Fixture) -> Result<PollSession, String> {
    fixture
        .journal
        .bootstrap_header_authority()
        .await
        .map_err(|error| error.to_string())?;
    let prepared = fixture
        .scope
        .prepare_session_create(&"b".repeat(32))
        .await
        .map_err(|error| error.to_string())?;
    let claim = fixture
        .scope
        .claim_session_create(prepared.local_nonce())
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The native intake session claim is absent.".to_owned())?;
    let bound = fixture
        .scope
        .record_created_session(&claim, "native-intake-session", 1)
        .await
        .map_err(|error| error.to_string())?;
    Ok(PollSession {
        intent_id: bound.intent_id(),
        allocation: bound.allocation().clone(),
        acquire_calls: std::rc::Rc::new(std::cell::Cell::new(0)),
        acknowledgement_calls: std::rc::Rc::new(std::cell::Cell::new(0)),
    })
}

async fn seed_native_inbox(
    scope: &crate::journal::ConnectionJournal,
    backlog_rows: usize,
) -> Result<(), String> {
    for message_id in 0..backlog_rows {
        let message_id =
            i64::try_from(message_id).map_err(|_| "The inbox ID is too large.".to_owned())?;
        scope
            .store_completion_inbox(message_id, "[]")
            .await
            .map_err(|error| {
                format!("The host could not save a completion inbox record: {error}.")
            })?;
    }
    Ok(())
}
