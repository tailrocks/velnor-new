use super::*;

const REFRESHED_SESSION_RESPONSE: &str = r#"{"sessionId":"session-1","messageQueueUrl":"https://queue-new.example/_apis/runtime/runnerscalesets/7/sessions/session-1/rotated/messages","messageQueueAccessToken":"replacement-queue-canary"}"#;
const ACQUIRED_RESPONSE: &str = r#"{"count":1,"value":[23]}"#;

#[test]
fn acquire_uses_service_origin_and_refresh_rebinds_before_replay() -> Result<(), &'static str> {
    let later = [
        Ok(exchange(200, TRUST_RUN_BODY)),
        Ok(exchange(401, "")),
        Ok(exchange(200, REFRESHED_SESSION_RESPONSE)),
        Ok(exchange(200, ACQUIRED_RESPONSE)),
    ];
    let (mut script, capability, mut session, batch) =
        capability_and_polled_session(AVAILABLE_TRUSTED, later)?;
    script.bind_github_api_origin().map_err(|_| "api origin")?;
    let trust = verify_test_offer(&mut script, &batch)?;
    let outcome = capability
        .acquire_verified(&mut script, &mut session, trust, &RefreshGate::new())
        .map_err(|_| "acquire after refresh")?;
    assert!(matches!(outcome, VerifiedAcquireOutcome::Acquired(_)));
    assert_eq!(script.seen.len(), 7);

    let acquire = &script.seen[4];
    let patch = &script.seen[5];
    let replay = &script.seen[6];
    assert_eq!(script.origins[3], "https://api.github.com");
    assert_eq!(
        script.origins[4],
        "https://pipelinesghubeus9.actions.githubusercontent.com"
    );
    assert_eq!(
        script.origins[5],
        "https://pipelinesghubeus9.actions.githubusercontent.com"
    );
    assert_eq!(
        script.origins[6],
        "https://pipelinesghubeus9.actions.githubusercontent.com"
    );
    assert_eq!(acquire.method, Method::Post);
    assert_eq!(acquire.path, crate::acquire_path(SET_ID));
    assert_eq!(acquire.query, replay.query);
    assert_eq!(acquire.body, b"[23]");
    assert_eq!(replay.method, acquire.method);
    assert_eq!(replay.path, acquire.path);
    assert_eq!(replay.body, acquire.body);
    assert_eq!(bearer(acquire), Some("Bearer queue-canary"));
    assert_eq!(bearer(replay), Some("Bearer replacement-queue-canary"));
    assert_eq!(patch.method, Method::Patch);
    assert_eq!(bearer(patch), Some("Bearer admin-canary"));
    assert_eq!(
        (acquire.purpose, acquire.bearer_role),
        (
            crate::RequestPurpose::AcquireJobs,
            crate::BearerRole::SessionQueue
        )
    );
    assert_eq!(
        (patch.purpose, patch.bearer_role),
        (
            crate::RequestPurpose::SessionRefresh,
            crate::BearerRole::ActionsAdmin
        )
    );
    assert_eq!(
        (replay.purpose, replay.bearer_role),
        (
            crate::RequestPurpose::AcquireJobs,
            crate::BearerRole::SessionQueue
        )
    );
    assert!(patch.path.ends_with("/sessions/session-1"));
    Ok(())
}
