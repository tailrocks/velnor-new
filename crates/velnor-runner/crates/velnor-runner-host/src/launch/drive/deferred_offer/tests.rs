use velnor_runner_github::{InnerJob, InnerKind, ParsedBatch, Poll};

use crate::{
    config::SourcePolicyMode,
    journal::{AssignmentKey, SourceAuthorizationResult},
    launch::{
        DeferredAuthorizedOffers,
        steps::source::test_receipt,
        turn::scoped_poll_tests::support::{
            Scratch, authorized_source, repository_connection, session_scope,
        },
    },
};

use super::DeferredAuthorizedOffer;

fn available(message_id: i64, request_id: i64) -> Poll {
    Poll::Batch(ParsedBatch {
        message_id,
        raw_body: String::new(),
        statistics: None,
        jobs: vec![InnerJob {
            kind: InnerKind::Available,
            request_id: Some(request_id),
            runner_id: None,
            runner_name: None,
            result: None,
            job_id: None,
            labels: Vec::new(),
            fields: Vec::new(),
            source: Default::default(),
        }],
    })
}

fn available_many(message_id: i64, request_ids: &[i64]) -> Poll {
    Poll::Batch(ParsedBatch {
        message_id,
        raw_body: String::new(),
        statistics: None,
        jobs: request_ids
            .iter()
            .map(|request_id| InnerJob {
                kind: InnerKind::Available,
                request_id: Some(*request_id),
                runner_id: None,
                runner_name: None,
                result: None,
                job_id: None,
                labels: Vec::new(),
                fields: Vec::new(),
                source: Default::default(),
            })
            .collect(),
    })
}

#[tokio::test]
async fn same_source_new_envelope_keeps_original_audit_id_and_binds_current_id()
-> Result<(), String> {
    let scratch = Scratch::new()?;
    let (scope, _intent_id, _allocation) = session_scope(&scratch).await?;
    let request_id = 91;
    let original_message_id = 17;
    let current_message_id = 18;
    let binding = scope
        .source_authorization_binding()
        .await
        .map_err(|error| error.to_string())?;
    let identity = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id })
        .await
        .map_err(|error| error.to_string())?;
    let original_receipt = test_receipt(
        &binding,
        request_id,
        original_message_id,
        authorized_source(),
        SourcePolicyMode::Repository,
    );
    assert_eq!(
        scope
            .record_source_authorization(&identity, &original_receipt)
            .await
            .map_err(|error| error.to_string())?,
        SourceAuthorizationResult::Recorded
    );
    let current_receipt = test_receipt(
        &binding,
        request_id,
        current_message_id,
        authorized_source(),
        SourcePolicyMode::Repository,
    );
    assert_eq!(
        scope
            .record_source_authorization(&identity, &current_receipt)
            .await
            .map_err(|error| error.to_string())?,
        SourceAuthorizationResult::Replayed
    );
    let set_id = binding.scale_set_id();
    let connection = repository_connection()?;
    let authorized = DeferredAuthorizedOffer::from_poll_receipt(
        &connection,
        &scope,
        set_id,
        &available(current_message_id, request_id),
        request_id,
        current_receipt,
    )
    .await
    .map_err(|error| format!("{error:?}"))?;

    assert_eq!(authorized.offer().message_id(), original_message_id);
    assert!(
        authorized
            .matches_current_envelope(&available(current_message_id, request_id), &scope, set_id)
            .await
    );
    assert!(
        !authorized
            .matches_current_envelope(
                &available(current_message_id + 1, request_id),
                &scope,
                set_id
            )
            .await
    );
    let unchanged = scope
        .authorized_offer(request_id, original_message_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "original durable offer disappeared".to_owned())?;
    assert_eq!(unchanged.message_id(), original_message_id);
    Ok(())
}

#[tokio::test]
async fn changed_source_cannot_bind_an_existing_offer() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let (scope, _intent_id, _allocation) = session_scope(&scratch).await?;
    let request_id = 92;
    let binding = scope
        .source_authorization_binding()
        .await
        .map_err(|error| error.to_string())?;
    let identity = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id })
        .await
        .map_err(|error| error.to_string())?;
    let original = test_receipt(
        &binding,
        request_id,
        27,
        authorized_source(),
        SourcePolicyMode::Repository,
    );
    scope
        .record_source_authorization(&identity, &original)
        .await
        .map_err(|error| error.to_string())?;
    let mut changed_source = authorized_source();
    changed_source.workflow_run_id += 1;
    let changed = test_receipt(
        &binding,
        request_id,
        28,
        changed_source,
        SourcePolicyMode::Repository,
    );
    assert!(
        scope
            .record_source_authorization(&identity, &changed)
            .await
            .is_err()
    );

    let connection = repository_connection()?;
    assert!(
        DeferredAuthorizedOffer::from_poll_receipt(
            &connection,
            &scope,
            binding.scale_set_id(),
            &available(28, request_id),
            request_id,
            changed,
        )
        .await
        .is_err()
    );
    let unchanged = scope
        .authorized_offer(request_id, 27)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "original durable offer disappeared".to_owned())?;
    assert_eq!(unchanged.message_id(), 27);
    Ok(())
}

#[tokio::test]
async fn zero_header_receipt_set_must_cover_every_available_request() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let (scope, _intent_id, _allocation) = session_scope(&scratch).await?;
    let request_ids = [91, 92];
    let message_id = 33;
    let binding = scope
        .source_authorization_binding()
        .await
        .map_err(|error| error.to_string())?;
    let mut receipts = Vec::new();
    for request_id in request_ids {
        let identity = scope
            .assignment_identity(AssignmentKey::AssignedRequest { request_id })
            .await
            .map_err(|error| error.to_string())?;
        let receipt = test_receipt(
            &binding,
            request_id,
            message_id,
            authorized_source(),
            SourcePolicyMode::Repository,
        );
        scope
            .record_source_authorization(&identity, &receipt)
            .await
            .map_err(|error| error.to_string())?;
        receipts.push((request_id, receipt));
    }
    let polled = available_many(message_id, &request_ids);
    let connection = repository_connection()?;
    assert!(
        DeferredAuthorizedOffers::from_receipts(
            &connection,
            &scope,
            binding.scale_set_id(),
            &polled,
            vec![receipts[0].clone()],
        )
        .await
        .is_err(),
        "A response with two requests needs two approvals."
    );
    let complete = DeferredAuthorizedOffers::from_receipts(
        &connection,
        &scope,
        binding.scale_set_id(),
        &polled,
        receipts,
    )
    .await
    .map_err(|error| format!("{error:?}"))?;
    assert!(
        complete
            .matches_current_poll(&polled, &scope, binding.scale_set_id())
            .await
    );
    assert!(
        !complete
            .matches_current_poll(
                &available_many(message_id, &[91, 93]),
                &scope,
                binding.scale_set_id(),
            )
            .await
    );
    Ok(())
}
