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
