use std::cell::Cell;

use super::*;

#[tokio::test]
async fn deferred_mode_persists_lifecycle_and_source_before_ack_without_acquire()
-> Result<(), String> {
    use crate::reconcile::LaunchPhase;

    let fixture = crate::journal::request_lifecycle::tests::native_started_intake_fixture()
        .await
        .map_err(|error| error.to_string())?;
    let scope = &fixture.scope;
    let request_id = 88;
    let polled = batch(
        71,
        vec![
            available(request_id),
            event(InnerKind::Assigned, fixture.request_id),
            InnerJob {
                kind: InnerKind::Started,
                request_id: Some(fixture.request_id),
                runner_id: Some(fixture.runner.id),
                runner_name: Some(fixture.runner.name.clone()),
                result: None,
                job_id: Some("native-started-job".to_owned()),
                labels: Vec::new(),
                fields: Vec::new(),
                source: fixture.source.clone(),
            },
        ],
    );
    let set_id = 501;
    let connection = native_fixture_connection()?;
    let identity = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id })
        .await
        .map_err(|error| error.to_string())?;
    let binding = scope
        .source_authorization_binding()
        .await
        .map_err(|error| error.to_string())?;
    let receipt = crate::launch::steps::source::test_receipt(
        &binding,
        request_id,
        71,
        authorized_source(),
        SourcePolicyMode::Repository,
    );
    scope
        .record_source_authorization(&identity, &receipt)
        .await
        .map_err(|error| error.to_string())?;
    let mut receipts = Some(
        DeferredAuthorizedOffers::from_receipts(
            &connection,
            scope,
            set_id,
            &polled,
            vec![(request_id, receipt)],
        )
        .await
        .map_err(|error| format!("Could not link the saved request: {error:?}"))?,
    );
    assert_ne!(
        scope
            .intent(fixture.intent_id)
            .await
            .map_err(|error| error.to_string())?
            .launch_phase,
        Some(LaunchPhase::RunnerStarted)
    );
    let ack_count = Cell::new(0);
    let authorize_count = Cell::new(0);
    let ack_saw_durable_lifecycle = Cell::new(false);
    let ack_saw_durable_offer = Cell::new(false);

    let (decision, acknowledged) = process_deferred_scoped_response(
        &scope,
        set_id,
        &polled,
        async |_| {
            authorize_count.set(authorize_count.get() + 1);
            receipts
                .take()
                .ok_or(crate::scale_set::EnsureError::AdmissionClosed)
        },
        async || {
            ack_count.set(ack_count.get() + 1);
            ack_saw_durable_lifecycle.set(
                !scope
                    .pending_completion_inbox(4_102_444_800, 10)
                    .await
                    .map_err(|_| crate::scale_set::EnsureError::AdmissionClosed)?
                    .is_empty(),
            );
            ack_saw_durable_lifecycle.set(
                ack_saw_durable_lifecycle.get()
                    && scope
                        .intent(fixture.intent_id)
                        .await
                        .map_err(|_| crate::scale_set::EnsureError::AdmissionClosed)?
                        .launch_phase
                        == Some(LaunchPhase::RunnerStarted),
            );
            ack_saw_durable_offer.set(
                scope
                    .authorized_offer(request_id, 71)
                    .await
                    .map_err(|_| crate::scale_set::EnsureError::AdmissionClosed)?
                    .is_some(),
            );
            Ok(())
        },
    )
    .await
    .map_err(|error| format!("Could not process the saved response: {error:?}"))?;

    assert!(acknowledged);
    assert!(!decision.completion_only);
    assert_eq!(authorize_count.get(), 1);
    assert_eq!(ack_count.get(), 1);
    assert!(ack_saw_durable_lifecycle.get());
    assert!(ack_saw_durable_offer.get());
    let request = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id })
        .await
        .map_err(|error| error.to_string())?;
    assert!(
        scope
            .reservation(&request)
            .await
            .map_err(|error| error.to_string())?
            .is_none()
    );
    Ok(())
}

#[tokio::test]
async fn assigned_only_response_is_observed_and_acked_without_acquisition() -> Result<(), String> {
    use crate::launch::CursorDisposition;

    let scratch = Scratch::new()?;
    let (scope, intent_id, allocation) = session_scope(&scratch).await?;
    let mut polled = batch(74, vec![event(InnerKind::Assigned, 93)]);
    if let Poll::Batch(batch) = &mut polled {
        batch.statistics = Some(velnor_runner_github::Statistics {
            total_available_jobs: 0,
            total_acquired_jobs: 0,
            total_assigned_jobs: 3,
            total_running_jobs: 0,
            total_registered_runners: 1,
            total_busy_runners: 0,
            total_idle_runners: 1,
        });
    }
    let attempt = scope
        .begin_zero_header_poll(intent_id, &allocation)
        .await
        .map_err(|error| error.to_string())?;
    let ack_count = Cell::new(0);
    let authorize_count = Cell::new(0);
    let (decision, acknowledged) = process_deferred_scoped_response(
        &scope,
        10_101,
        &polled,
        async |_| {
            authorize_count.set(authorize_count.get() + 1);
            Ok(DeferredAuthorizedOffers::empty())
        },
        async || {
            ack_count.set(ack_count.get() + 1);
            Ok(())
        },
    )
    .await
    .map_err(|error| format!("Could not process the Assigned response: {error:?}"))?;
    assert!(acknowledged);
    assert!(decision.completion_only);
    assert_eq!(ack_count.get(), 1);
    assert_eq!(authorize_count.get(), 0);

    let outcome = finish_scoped_poll_response(
        &scope,
        intent_id,
        10_101,
        "session-scoped-poll",
        &allocation,
        &polled,
        attempt,
        async { Ok((decision, acknowledged)) },
        0,
    )
    .await
    .map_err(|error| error.to_string())?;
    assert_eq!(outcome.assigned_total, Some(3));
    assert_eq!(outcome.cursor_disposition, CursorDisposition::ResetToHead);
    assert!(!outcome.session_delete_candidate);
    Ok(())
}

#[tokio::test]
async fn assigned_lifecycle_ack_failure_settles_without_delete_candidate() -> Result<(), String> {
    use crate::launch::CursorDisposition;

    let scratch = Scratch::new()?;
    let (scope, intent_id, allocation) = session_scope(&scratch).await?;
    let polled = batch(75, vec![event(InnerKind::Assigned, 94)]);
    let attempt = scope
        .begin_zero_header_poll(intent_id, &allocation)
        .await
        .map_err(|error| error.to_string())?;
    let (decision, acknowledged) = process_deferred_scoped_response(
        &scope,
        10_101,
        &polled,
        async |_| Ok(DeferredAuthorizedOffers::empty()),
        async || Err(crate::scale_set::EnsureError::Uncertain),
    )
    .await
    .map_err(|error| format!("Could not process the Assigned response: {error:?}"))?;
    assert!(!acknowledged);

    let outcome = finish_scoped_poll_response(
        &scope,
        intent_id,
        10_101,
        "session-scoped-poll",
        &allocation,
        &polled,
        attempt,
        async { Ok((decision, acknowledged)) },
        0,
    )
    .await
    .map_err(|error| error.to_string())?;
    assert_eq!(
        outcome.cursor_disposition,
        CursorDisposition::Deferred { message_id: 75 }
    );
    assert!(!outcome.session_delete_candidate);
    Ok(())
}

#[tokio::test]
async fn deferred_mode_rejects_partial_receipts_and_settles_known_get() -> Result<(), String> {
    use crate::launch::CursorDisposition;

    let scratch = Scratch::new()?;
    let (scope, intent_id, allocation) = session_scope(&scratch).await?;
    let mut polled = batch(72, vec![available(89)]);
    if let Poll::Batch(batch) = &mut polled {
        batch.statistics = Some(velnor_runner_github::Statistics {
            total_available_jobs: 1,
            total_acquired_jobs: 0,
            total_assigned_jobs: 11,
            total_running_jobs: 0,
            total_registered_runners: 1,
            total_busy_runners: 0,
            total_idle_runners: 1,
        });
    }
    let attempt = scope
        .begin_zero_header_poll(intent_id, &allocation)
        .await
        .map_err(|error| error.to_string())?;
    let ack_count = Cell::new(0);
    let (decision, acknowledged) = process_deferred_scoped_response(
        &scope,
        10_101,
        &polled,
        async |_| Ok(DeferredAuthorizedOffers::empty()),
        async || {
            ack_count.set(ack_count.get() + 1);
            Ok(())
        },
    )
    .await
    .map_err(|error| format!("Could not process the saved response: {error:?}"))?;
    assert!(!acknowledged);
    assert!(!decision.completion_only);
    assert_eq!(ack_count.get(), 0);

    let outcome = finish_scoped_poll_response(
        &scope,
        intent_id,
        10_101,
        "session-scoped-poll",
        &allocation,
        &polled,
        attempt,
        async { Ok((decision, acknowledged)) },
        0,
    )
    .await
    .map_err(|error| format!("Could not complete the GET operation: {error:?}"))?;
    assert_eq!(
        outcome.cursor_disposition,
        CursorDisposition::Deferred { message_id: 72 }
    );
    assert_eq!(outcome.assigned_total, Some(11));
    let next = scope
        .begin_zero_header_poll(intent_id, &allocation)
        .await
        .map_err(|error| format!("Could not repeat the GET operation: {error:?}"))?;
    assert_eq!(next.header_value(), 0);
    Ok(())
}

#[tokio::test]
async fn deferred_mode_never_acks_available_with_unsupported_sibling() -> Result<(), String> {
    for kind in [InnerKind::Unsupported("future-kind".to_owned())] {
        let scratch = Scratch::new()?;
        let (scope, _, _) = session_scope(&scratch).await?;
        let polled = batch(73, vec![available(90), event(kind, 91)]);
        let ack_count = Cell::new(0);
        let authorize_count = Cell::new(0);
        let (decision, acknowledged) = process_deferred_scoped_response(
            &scope,
            10_101,
            &polled,
            async |_| {
                authorize_count.set(authorize_count.get() + 1);
                Ok(DeferredAuthorizedOffers::empty())
            },
            async || {
                ack_count.set(ack_count.get() + 1);
                Ok(())
            },
        )
        .await
        .map_err(|error| format!("Could not process the saved response: {error:?}"))?;
        assert!(!acknowledged);
        assert!(!decision.completion_only);
        assert_eq!(authorize_count.get(), 0);
        assert_eq!(ack_count.get(), 0);
        let request = scope
            .assignment_identity(AssignmentKey::AssignedRequest { request_id: 90 })
            .await
            .map_err(|error| error.to_string())?;
        assert!(
            scope
                .reservation(&request)
                .await
                .map_err(|error| error.to_string())?
                .is_none()
        );
    }
    Ok(())
}

#[tokio::test]
async fn deferred_mode_authorization_error_settles_get_for_same_session_retry() -> Result<(), String>
{
    let scratch = Scratch::new()?;
    let (scope, intent_id, allocation) = session_scope(&scratch).await?;
    let polled = batch(74, vec![available(92)]);
    let attempt = scope
        .begin_zero_header_poll(intent_id, &allocation)
        .await
        .map_err(|error| error.to_string())?;
    let ack_count = Cell::new(0);
    let (decision, acknowledged) = process_deferred_scoped_response(
        &scope,
        10_101,
        &polled,
        async |_| Err(crate::scale_set::EnsureError::AdmissionClosed),
        async || {
            ack_count.set(ack_count.get() + 1);
            Ok(())
        },
    )
    .await
    .map_err(|error| format!("The GET result was uncertain: {error:?}"))?;
    assert!(!acknowledged);
    assert_eq!(ack_count.get(), 0);
    let _outcome = finish_scoped_poll_response(
        &scope,
        intent_id,
        10_101,
        "session-scoped-poll",
        &allocation,
        &polled,
        attempt,
        async { Ok((decision, acknowledged)) },
        0,
    )
    .await
    .map_err(|error| format!("Could not complete the GET operation: {error:?}"))?;
    assert!(
        scope
            .begin_zero_header_poll(intent_id, &allocation)
            .await
            .is_ok()
    );
    Ok(())
}

#[tokio::test]
async fn deferred_mode_ack_failure_settles_get_and_keeps_authorized_offer() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let (scope, intent_id, allocation) = session_scope(&scratch).await?;
    let polled = batch(75, vec![available(93)]);
    let receipts = Some(durable_offer_set(&polled, &scope, 10_101, 93).await?);
    let mut receipts = receipts;
    let attempt = scope
        .begin_zero_header_poll(intent_id, &allocation)
        .await
        .map_err(|error| error.to_string())?;
    let (decision, acknowledged) = process_deferred_scoped_response(
        &scope,
        10_101,
        &polled,
        async |_| {
            receipts
                .take()
                .ok_or(crate::scale_set::EnsureError::AdmissionClosed)
        },
        async || Err(crate::scale_set::EnsureError::Uncertain),
    )
    .await
    .map_err(|error| format!("The GET result was uncertain: {error:?}"))?;
    assert!(!acknowledged);
    let _outcome = finish_scoped_poll_response(
        &scope,
        intent_id,
        10_101,
        "session-scoped-poll",
        &allocation,
        &polled,
        attempt,
        async { Ok((decision, acknowledged)) },
        0,
    )
    .await
    .map_err(|error| format!("Could not complete the GET operation: {error:?}"))?;
    assert!(
        scope
            .authorized_offer(93, 75)
            .await
            .map_err(|error| error.to_string())?
            .is_some()
    );
    assert!(
        scope
            .begin_zero_header_poll(intent_id, &allocation)
            .await
            .is_ok()
    );
    Ok(())
}
