use super::*;

#[tokio::test]
async fn exact_refresh_receipt_builds_next_poll_with_new_memory_credentials() -> Result<(), String>
{
    let scratch = Scratch::new()?;
    let session_id = "session-refresh-exact";
    let (journal, scope, intent_id, allocation) = bound_session(&scratch, 0, session_id).await?;
    let mut transport = RefreshTransport::new([
        Ok(Exchange {
            status: 200,
            body: refreshed_body(session_id, "messages-new", "queue-token-new"),
        }),
        Ok(Exchange {
            status: 202,
            body: Vec::new(),
        }),
    ]);

    let refreshed = refresh_bound_connection_session_with(
        &mut transport,
        "admin-token",
        "https://admin.example.test",
        &scope,
        intent_id,
        allocation.scale_set_id(),
        session_id,
    )
    .await
    .map_err(|error| error.to_string())?;
    let poll_result = poll(
        &mut transport,
        "messages-new",
        0,
        0,
        refreshed.token(),
        &RefreshGate::new(),
        || Ok::<(), WireError>(()),
    );

    assert!(matches!(poll_result, Ok(Poll::Empty)));
    assert_eq!(transport.requests.len(), 2);
    assert_eq!(transport.requests[0].method, Method::Patch);
    assert_eq!(
        transport.requests[0].path,
        format!(
            "_apis/runtime/runnerscalesets/{}/sessions/{session_id}",
            allocation.scale_set_id()
        )
    );
    assert!(transport.requests[0].headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("authorization") && value == "Bearer admin-token"
    }));
    assert_eq!(transport.requests[1].method, Method::Get);
    assert_eq!(transport.requests[1].path, "messages-new");
    assert!(transport.requests[1].headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("authorization") && value == "Bearer queue-token-new"
    }));
    let records = scope
        .session_refreshes(intent_id)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].state(), SessionRefreshState::Succeeded);
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
async fn returned_transport_uncertainty_allows_only_a_new_durable_generation() -> Result<(), String>
{
    let scratch = Scratch::new()?;
    let session_id = "session-refresh-retry";
    let (_journal, scope, intent_id, allocation) = bound_session(&scratch, 0, session_id).await?;
    let mut first = RefreshTransport::new([Err(TransportFail::Reset)]);
    let first_result = refresh_bound_connection_session_with(
        &mut first,
        "admin-token",
        "https://admin.example.test",
        &scope,
        intent_id,
        allocation.scale_set_id(),
        session_id,
    )
    .await;
    assert!(matches!(
        first_result,
        Err(crate::scale_set::EnsureError::Uncertain)
    ));
    assert_eq!(first.requests.len(), 1);

    let mut second = RefreshTransport::new([Ok(Exchange {
        status: 200,
        body: refreshed_body(session_id, "messages-next", "queue-token-next"),
    })]);
    let refreshed = refresh_bound_connection_session_with(
        &mut second,
        "admin-token",
        "https://admin.example.test",
        &scope,
        intent_id,
        allocation.scale_set_id(),
        session_id,
    )
    .await
    .map_err(|error| error.to_string())?;
    assert_eq!(refreshed.session_id, session_id);
    assert_eq!(second.requests.len(), 1);
    assert_eq!(second.requests[0].method, Method::Patch);
    let records = scope
        .session_refreshes(intent_id)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].generation(), 1);
    assert_eq!(records[0].state(), SessionRefreshState::TransportError);
    assert_eq!(records[1].generation(), 2);
    assert_eq!(records[1].state(), SessionRefreshState::Succeeded);
    Ok(())
}

#[tokio::test]
async fn incorrect_id_or_queue_route_stays_uncertain_and_never_installs_credentials()
-> Result<(), String> {
    for (name, returned_id, queue_url) in [
        ("incorrect-id", "foreign-session", "messages"),
        (
            "incorrect-route",
            "session-refresh-invalid",
            "http://queue.example/messages",
        ),
    ] {
        let scratch = Scratch::new()?;
        let session_id = if name == "incorrect-id" {
            "session-refresh-expected"
        } else {
            "session-refresh-invalid"
        };
        let (_journal, scope, intent_id, allocation) =
            bound_session(&scratch, 0, session_id).await?;
        let mut transport = RefreshTransport::new([Ok(Exchange {
            status: 200,
            body: refreshed_body(returned_id, queue_url, "must-not-install"),
        })]);
        let result = refresh_bound_connection_session_with(
            &mut transport,
            "admin-token",
            "https://admin.example.test",
            &scope,
            intent_id,
            allocation.scale_set_id(),
            session_id,
        )
        .await;

        assert!(result.is_err(), "{name} must not return queue credentials");
        assert_eq!(transport.requests.len(), 1);
        let records = scope
            .session_refreshes(intent_id)
            .await
            .map_err(|error| error.to_string())?;
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].state(), SessionRefreshState::Uncertain);
        assert!(
            scope
                .claim_session_refresh(intent_id, session_id)
                .await
                .map_err(|error| error.to_string())?
                .is_none()
        );
    }
    Ok(())
}

#[tokio::test]
async fn rejected_refresh_and_wrong_durable_id_send_no_retryable_request() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let session_id = "session-refresh-rejected";
    let (_journal, scope, intent_id, allocation) = bound_session(&scratch, 0, session_id).await?;
    let mut transport = RefreshTransport::new([Ok(Exchange {
        status: 409,
        body: Vec::new(),
    })]);
    let result = refresh_bound_connection_session_with(
        &mut transport,
        "admin-token",
        "https://admin.example.test",
        &scope,
        intent_id,
        allocation.scale_set_id(),
        session_id,
    )
    .await;
    assert!(matches!(
        result,
        Err(crate::scale_set::EnsureError::Conflict)
    ));
    assert_eq!(transport.requests.len(), 1);
    let records = scope
        .session_refreshes(intent_id)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(records[0].state(), SessionRefreshState::Uncertain);
    assert!(
        scope
            .claim_session_refresh(intent_id, session_id)
            .await
            .map_err(|error| error.to_string())?
            .is_none()
    );

    let mut wrong_id_transport = RefreshTransport::new([]);
    assert!(
        refresh_bound_connection_session_with(
            &mut wrong_id_transport,
            "admin-token",
            "https://admin.example.test",
            &scope,
            intent_id,
            allocation.scale_set_id(),
            "different-session",
        )
        .await
        .is_err()
    );
    assert_eq!(wrong_id_transport.requests.as_slice(), &[]);
    Ok(())
}
