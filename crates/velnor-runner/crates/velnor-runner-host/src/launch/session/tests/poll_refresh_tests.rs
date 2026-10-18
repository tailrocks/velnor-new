use crate::listen::{PollPathResult, poll_path_with, queue_path};
use crate::scale_set::EnsureError;
use velnor_runner_github::{Exchange, Method, Poll};

use super::{RefreshTransport, Scratch, bound_session, refresh_bound_connection_session_with};

#[tokio::test]
async fn expired_poll_refreshes_exact_session_then_builds_get_with_new_credentials()
-> Result<(), String> {
    let scratch = Scratch::new()?;
    let session_id = "session-poll-refresh";
    let (journal, scope, intent_id, allocation) = bound_session(&scratch, 0, session_id).await?;
    let mut transport = RefreshTransport::new([
        Ok(Exchange {
            status: 401,
            body: Vec::new(),
        }),
        Ok(Exchange {
            status: 200,
            body: format!(
                r#"{{"sessionId":"{session_id}","messageQueueUrl":"messages-new","messageQueueAccessToken":"queue-token-new"}}"#
            )
            .into_bytes(),
        }),
        Ok(Exchange {
            status: 202,
            body: Vec::new(),
        }),
    ]);

    let expired = poll_path_with(&mut transport, "queue-token-old", "messages-old", 0, 0);
    assert_eq!(expired, Ok(PollPathResult::RefreshRequired));
    assert_eq!(transport.requests.len(), 1);
    assert_eq!(transport.requests[0].method, Method::Get);
    assert_eq!(transport.requests[0].path, "messages-old");
    assert!(transport.requests[0].headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("authorization") && value == "Bearer queue-token-old"
    }));

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
    assert_eq!(refreshed.session_id, session_id);
    let path = queue_path("https://admin.example.test", &refreshed.message_queue_url)
        .ok_or_else(|| "refreshed queue route was not relative to the admin origin".to_owned())?;
    let rebuilt = poll_path_with(&mut transport, refreshed.token(), path, 0, 0);

    assert_eq!(rebuilt, Ok(PollPathResult::Response(Poll::Empty)));
    assert_eq!(transport.requests.len(), 3);
    assert_eq!(transport.requests[1].method, Method::Patch);
    assert_eq!(
        transport.requests[1].path,
        format!(
            "_apis/runtime/runnerscalesets/{}/sessions/{session_id}",
            allocation.scale_set_id()
        )
    );
    assert!(transport.requests[1].headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("authorization") && value == "Bearer admin-token"
    }));
    assert_eq!(transport.requests[2].method, Method::Get);
    assert_eq!(transport.requests[2].path, "messages-new");
    assert!(transport.requests[2].headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("authorization") && value == "Bearer queue-token-new"
    }));
    let refreshes = scope
        .session_refreshes(intent_id)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(refreshes.len(), 1);
    assert_eq!(
        refreshes[0].state(),
        crate::journal::SessionRefreshState::Succeeded
    );
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
async fn failed_refresh_keeps_old_poll_unreplayed_and_durable_refresh_held() -> Result<(), String> {
    let scratch = Scratch::new()?;
    let session_id = "session-poll-refresh-failure";
    let (_journal, scope, intent_id, allocation) = bound_session(&scratch, 0, session_id).await?;
    let mut transport = RefreshTransport::new([
        Ok(Exchange {
            status: 401,
            body: Vec::new(),
        }),
        Err(velnor_runner_github::TransportFail::Reset),
    ]);

    assert_eq!(
        poll_path_with(&mut transport, "queue-token-old", "messages-old", 0, 0),
        Ok(PollPathResult::RefreshRequired)
    );
    let refresh = refresh_bound_connection_session_with(
        &mut transport,
        "admin-token",
        "https://admin.example.test",
        &scope,
        intent_id,
        allocation.scale_set_id(),
        session_id,
    )
    .await;

    assert!(matches!(refresh, Err(EnsureError::Uncertain)));
    assert_eq!(transport.requests.len(), 2);
    assert_eq!(transport.requests[0].method, Method::Get);
    assert_eq!(transport.requests[1].method, Method::Patch);
    assert_eq!(
        transport
            .requests
            .iter()
            .filter(|request| request.method == Method::Get && request.path == "messages-old")
            .count(),
        1
    );
    let refreshes = scope
        .session_refreshes(intent_id)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(refreshes.len(), 1);
    assert_eq!(
        refreshes[0].state(),
        crate::journal::SessionRefreshState::TransportError
    );
    Ok(())
}
