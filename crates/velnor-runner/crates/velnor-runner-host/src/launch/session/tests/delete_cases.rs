use super::*;

async fn settle_zero_poll(
    scope: &ConnectionJournal,
    intent_id: i64,
    allocation: &HeaderAllocation,
    session_id: &str,
) -> Result<(), String> {
    let attempt = scope
        .begin_zero_header_poll(intent_id, allocation)
        .await
        .map_err(|error| error.to_string())?;
    let proof = CompletedHeaderPoll::after_response(
        attempt,
        allocation,
        scope,
        allocation.scale_set_id(),
        session_id,
    )
    .map_err(|error| error.to_string())?;
    scope
        .settle_session_poll(intent_id, proof)
        .await
        .map_err(|error| error.to_string())
}

#[tokio::test]
async fn zero_header_attempt_produces_proof_without_releasing_positive_charge() -> Result<(), String>
{
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 1_000,
            memory_bytes: 1_073_741_824,
        })
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bootstrap_header_authority()
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_engine("test-engine")
        .await
        .map_err(|error| error.to_string())?;
    let binding = binding()?;
    let scope = journal
        .connection_scope(&binding)
        .await
        .map_err(|error| error.to_string())?;
    scope
        .bind_scale_set(10_101, "integration", SetOwnership::Adopted)
        .await
        .map_err(|error| error.to_string())?;
    let prepared = scope
        .prepare_session_create("0123456789abcdef0123456789abcdef")
        .await
        .map_err(|error| error.to_string())?;
    let claim = scope
        .claim_session_create(prepared.local_nonce())
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "session create claim missing".to_owned())?;
    let bound = scope
        .record_created_session(&claim, "session-zero-proof", 1)
        .await
        .map_err(|error| error.to_string())?;
    let allocation = bound.allocation().clone();
    assert_eq!(allocation.target(), 1);

    let attempt = scope
        .begin_zero_header_poll(bound.intent_id(), &allocation)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(attempt.header_value(), 0);
    let proof = CompletedHeaderPoll::after_response(
        attempt,
        &allocation,
        &scope,
        10_101,
        "session-zero-proof",
    )
    .map_err(|error| error.to_string())?;
    scope
        .settle_session_poll(bound.intent_id(), proof)
        .await
        .map_err(|error| error.to_string())?;

    assert_eq!(
        journal
            .charged_header_capacity()
            .await
            .map_err(|error| error.to_string())?,
        1
    );
    let current = scope
        .session_header_allocation(bound.intent_id())
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(current.target(), 1);
    Ok(())
}

#[tokio::test]
async fn exact_204_deletes_only_the_bound_session_and_retires_its_charge() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let session_id = "session-exact-delete";
    let (journal, scope, intent_id, allocation) = bound_session(&scratch, 1, session_id).await?;
    settle_zero_poll(&scope, intent_id, &allocation, session_id).await?;
    let session = velnor_runner_github::create_session(
            &mut DeleteTransport {
                status: 200,
                body: format!(
                    r#"{{"sessionId":"{session_id}","messageQueueUrl":"https://queue.example/messages","messageQueueAccessToken":"queue-token"}}"#
                )
                .into_bytes(),
                calls: Vec::new(),
            },
            allocation.scale_set_id(),
            "velnor-host",
            "unused-admin-token",
        )
        .map_err(|error| error.to_string())?;
    assert_eq!(session.session_id, session_id);

    let mut transport = DeleteTransport {
        status: 204,
        body: Vec::new(),
        calls: Vec::new(),
    };
    let outcome = delete_removed_connection_session_with(
        &mut transport,
        "admin-token",
        &session,
        &scope,
        intent_id,
        &allocation,
    )
    .await;

    assert_eq!(outcome, SessionDeleteOutcome::Deleted);
    assert_eq!(
        transport.calls,
        vec![(
            Method::Delete,
            format!(
                "_apis/runtime/runnerscalesets/{}/sessions/{session_id}",
                allocation.scale_set_id()
            ),
            Some("api-version=6.0-preview".to_owned()),
            Some("Bearer admin-token".to_owned()),
        )]
    );
    assert!(scope.session_header_allocation(intent_id).await.is_err());
    assert_eq!(
        journal
            .charged_header_capacity()
            .await
            .map_err(|error| error.to_string())?,
        0
    );
    Ok(())
}

#[tokio::test]
async fn delete_is_not_sent_while_durable_session_work_remains() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let session_id = "session-with-pending-work";
    let (journal, scope, intent_id, allocation) = bound_session(&scratch, 1, session_id).await?;
    settle_zero_poll(&scope, intent_id, &allocation, session_id).await?;
    scope
        .store_completion_inbox(44, "pending lifecycle event")
        .await
        .map_err(|error| error.to_string())?;
    let session = velnor_runner_github::create_session(
            &mut DeleteTransport {
                status: 200,
                body: format!(
                    r#"{{"sessionId":"{session_id}","messageQueueUrl":"https://queue.example/messages","messageQueueAccessToken":"queue-token"}}"#
                )
                .into_bytes(),
                calls: Vec::new(),
            },
            allocation.scale_set_id(),
            "velnor-host",
            "unused-admin-token",
        )
        .map_err(|error| error.to_string())?;
    let mut transport = DeleteTransport {
        status: 204,
        body: Vec::new(),
        calls: Vec::new(),
    };

    assert_eq!(
        delete_removed_connection_session_with(
            &mut transport,
            "admin-token",
            &session,
            &scope,
            intent_id,
            &allocation,
        )
        .await,
        SessionDeleteOutcome::NotReady
    );
    assert_eq!(transport.calls.as_slice(), &[]);
    assert_eq!(
        journal
            .charged_header_capacity()
            .await
            .map_err(|error| error.to_string())?,
        1
    );
    Ok(())
}

#[tokio::test]
async fn non_204_delete_holds_charge_and_does_not_retry() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let session_id = "session-delete-uncertain";
    let (journal, scope, intent_id, allocation) = bound_session(&scratch, 1, session_id).await?;
    settle_zero_poll(&scope, intent_id, &allocation, session_id).await?;
    let session = velnor_runner_github::create_session(
            &mut DeleteTransport {
                status: 200,
                body: format!(
                    r#"{{"sessionId":"{session_id}","messageQueueUrl":"https://queue.example/messages","messageQueueAccessToken":"queue-token"}}"#
                )
                .into_bytes(),
                calls: Vec::new(),
            },
            allocation.scale_set_id(),
            "velnor-host",
            "unused-admin-token",
        )
        .map_err(|error| error.to_string())?;
    let mut transport = DeleteTransport {
        status: 200,
        body: Vec::new(),
        calls: Vec::new(),
    };
    assert_eq!(
        delete_removed_connection_session_with(
            &mut transport,
            "admin-token",
            &session,
            &scope,
            intent_id,
            &allocation,
        )
        .await,
        SessionDeleteOutcome::Held
    );
    assert_eq!(transport.calls.len(), 1);
    assert_eq!(
        journal
            .charged_header_capacity()
            .await
            .map_err(|error| error.to_string())?,
        1
    );
    assert_eq!(
        delete_removed_connection_session_with(
            &mut transport,
            "admin-token",
            &session,
            &scope,
            intent_id,
            &allocation,
        )
        .await,
        SessionDeleteOutcome::NotReady
    );
    assert_eq!(transport.calls.len(), 1);
    Ok(())
}
