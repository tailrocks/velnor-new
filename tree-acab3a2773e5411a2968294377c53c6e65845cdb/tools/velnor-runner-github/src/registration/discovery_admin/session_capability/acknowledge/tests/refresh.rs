use super::*;

fn route_refreshed_queue(
    transport: &mut Script,
    queue_url: &str,
    request: &mut SessionRequest,
) -> Result<String, SessionError> {
    let rest = queue_url
        .strip_prefix("https://")
        .ok_or(SessionError::Uncertain)?;
    let (host, path) = rest.split_once('/').ok_or(SessionError::Uncertain)?;
    if host.is_empty() || host.contains('@') || host.chars().any(char::is_whitespace) {
        return Err(SessionError::Uncertain);
    }
    transport.origin = format!("https://{host}");
    let base_path = format!("/{path}");
    request.path.clone_from(&base_path);
    Ok(base_path)
}

#[test]
fn refreshed_ack_retries_exact_message_path_and_next_poll_uses_cached_queue_base()
-> Result<(), &'static str> {
    let (mut script, capability, mut session, message_id) =
        test_capability_and_session_with_replies(
            STARTED_MESSAGE,
            vec![
                Ok(exchange(401, "")),
                Ok(exchange(200, REFRESHED_SESSION_RESPONSE)),
                Ok(exchange(204, "")),
                Ok(exchange(202, "")),
            ],
        )?;
    assert_eq!(message_id, 17);
    let result = capability
        .acknowledge_resolved_message(
            &mut script,
            &mut session,
            message_id,
            true,
            &RefreshGate::new(),
            route_refreshed_queue,
        )
        .map_err(|_| "ack after refresh")?;
    assert_eq!(result, Ack::Deleted);
    assert_ack_refresh_exchange(&script, message_id);
    assert_eq!(
        session.message_queue_url(),
        "https://queue-new.example/_apis/runtime/runnerscalesets/7/sessions/session-1/rotated/messages"
    );

    let next = capability
        .poll_with_trust(
            &mut script,
            &mut session,
            message_id,
            1,
            &RefreshGate::new(),
            route_refreshed_queue,
        )
        .map_err(|_| "poll after refreshed ack")?;
    assert!(matches!(next, PollWithTrust::Empty));
    assert_cached_queue_poll(&script);
    Ok(())
}

fn assert_ack_refresh_exchange(script: &Script, message_id: i64) {
    assert_eq!(script.seen.len(), 6);
    let first_delete = &script.seen[3];
    let patch = &script.seen[4];
    let replay_delete = &script.seen[5];
    assert_eq!(script.origins[3], "https://queue.example");
    assert_eq!(
        script.origins[4],
        "https://pipelinesghubeus9.actions.githubusercontent.com"
    );
    assert_eq!(script.origins[5], "https://queue-new.example");
    assert_eq!(first_delete.method, Method::Delete);
    assert_eq!(first_delete.path, format!("{MESSAGE_PATH}/{message_id}"));
    assert_eq!(patch.method, Method::Patch);
    assert_eq!(
        patch.path,
        "_apis/runtime/runnerscalesets/7/sessions/session-1"
    );
    assert_eq!(authorization(patch), Some("Bearer admin-canary"));
    assert_eq!(replay_delete.method, Method::Delete);
    assert_eq!(
        replay_delete.path,
        format!("{REFRESHED_MESSAGE_PATH}/{message_id}")
    );
    assert_eq!(replay_delete.query, first_delete.query);
    assert_eq!(replay_delete.body, first_delete.body);
    assert_eq!(authorization(first_delete), Some("Bearer queue-canary"));
    assert_eq!(
        authorization(replay_delete),
        Some("Bearer replacement-queue-canary")
    );
}

fn assert_cached_queue_poll(script: &Script) {
    assert_eq!(script.seen.len(), 7);
    assert_eq!(script.origins[6], "https://queue-new.example");
    assert_eq!(script.seen[6].method, Method::Get);
    assert_eq!(script.seen[6].path, REFRESHED_MESSAGE_PATH);
    assert_eq!(
        authorization(&script.seen[6]),
        Some("Bearer replacement-queue-canary")
    );
}

fn authorization(request: &SessionRequest) -> Option<&str> {
    request
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
        .map(|(_, value)| value.as_str())
}

#[test]
fn uncertain_refreshed_ack_remains_one_shot_and_does_not_advance_queue() -> Result<(), &'static str>
{
    let (mut script, capability, mut session, message_id) =
        test_capability_and_session_with_replies(
            STARTED_MESSAGE,
            vec![
                Ok(exchange(401, "")),
                Ok(exchange(200, REFRESHED_SESSION_RESPONSE)),
                Err(TransportFail::Reset),
            ],
        )?;
    assert_eq!(
        capability.acknowledge_resolved_message(
            &mut script,
            &mut session,
            message_id,
            true,
            &RefreshGate::new(),
            route_refreshed_queue,
        ),
        Err(SessionError::Uncertain)
    );
    assert_eq!(script.seen.len(), 6);
    assert_eq!(script.seen[5].method, Method::Delete);
    assert_eq!(
        script.seen[5].path,
        format!("{REFRESHED_MESSAGE_PATH}/{message_id}")
    );
    assert_eq!(
        session.message_queue_url(),
        "https://queue-new.example/_apis/runtime/runnerscalesets/7/sessions/session-1/rotated/messages"
    );

    assert!(
        capability
            .acknowledge_resolved_message(
                &mut script,
                &mut session,
                message_id,
                true,
                &RefreshGate::new(),
                route_refreshed_queue,
            )
            .is_err()
    );
    assert!(
        capability
            .poll_with_trust(
                &mut script,
                &mut session,
                message_id,
                1,
                &RefreshGate::new(),
                route_refreshed_queue,
            )
            .is_err()
    );
    assert_eq!(script.seen.len(), 6);
    Ok(())
}
