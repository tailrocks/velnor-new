//! Session create and refresh. No sockets.

use std::collections::VecDeque;

use velnor_runner_github::{
    Certainty, Exchange, Method, SessionError, SessionRequest, Transport, TransportFail, WireError,
    create_session, refresh_if_current, refresh_session,
};

const BODY: &str = r#"{"sessionId":"sess","messageQueueUrl":"_apis/runtime/runnerscalesets/7/sessions/sess/messages","messageQueueAccessToken":"queue-token-canary","ownerName":"velnor-host"}"#;

struct Script {
    replies: VecDeque<Result<Exchange, TransportFail>>,
    seen: Vec<SessionRequest>,
}

impl Script {
    fn once(status: u16, body: &str) -> Self {
        Self {
            replies: VecDeque::from(vec![Ok(Exchange {
                status,
                body: body.as_bytes().to_vec(),
            })]),
            seen: Vec::new(),
        }
    }
}

impl Transport for Script {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.seen.push(request.clone());
        self.replies.pop_front().ok_or(TransportFail::Reset)?
    }
}

#[test]
fn create_stores_the_queue_and_hides_the_token() -> Result<(), &'static str> {
    let mut script = Script::once(200, BODY);
    let created =
        create_session(&mut script, 7, "velnor-host", "admin-canary").map_err(|_| "create")?;
    assert_eq!(created.session_id, "sess");
    assert!(created.message_queue_url.contains("/messages"));
    assert_eq!(created.token(), "queue-token-canary");
    assert!(created.statistics().is_none());
    let rendered = format!("{created:?} {:?}", script.seen[0]);
    assert!(!rendered.contains("queue-token-canary"));
    assert!(!rendered.contains("admin-canary"));
    assert_eq!(script.seen[0].method, Method::Post);
    assert!(script.seen[0].path.ends_with("/7/sessions"));
    assert_eq!(
        script.seen[0].query.as_deref(),
        Some("api-version=6.0-preview")
    );
    assert!(
        script.seen[0]
            .body
            .windows(9)
            .any(|part| part == b"ownerName")
    );
    Ok(())
}

#[test]
fn create_keeps_session_statistics() -> Result<(), &'static str> {
    let body = r#"{"sessionId":"sess","messageQueueUrl":"_apis/runtime/runnerscalesets/7/sessions/sess/messages","messageQueueAccessToken":"queue-token-canary","statistics":{"totalAvailableJobs":2,"totalAcquiredJobs":1,"totalAssignedJobs":4,"totalRunningJobs":3,"totalRegisteredRunners":5,"totalBusyRunners":6,"totalIdleRunners":7}}"#;
    let mut script = Script::once(200, body);
    let created =
        create_session(&mut script, 7, "velnor-host", "admin-canary").map_err(|_| "create")?;
    let stats = created.statistics().ok_or("statistics")?;
    assert_eq!(stats.total_available_jobs, 2);
    assert_eq!(stats.total_acquired_jobs, 1);
    assert_eq!(stats.assigned_population(), 4);
    assert_eq!(stats.total_running_jobs, 3);
    assert_eq!(stats.total_registered_runners, 5);
    assert_eq!(stats.total_busy_runners, 6);
    assert_eq!(stats.total_idle_runners, 7);
    assert!(!format!("{created:?}").contains("queue-token-canary"));
    Ok(())
}

#[test]
fn conflict_does_not_delete_the_other_session() -> Result<(), &'static str> {
    let mut script = Script::once(409, "other-session");
    let err = create_session(&mut script, 7, "velnor-host", "admin-canary");
    match err {
        Err(error) => {
            assert_eq!(error, SessionError::Conflict);
            assert_eq!(error.certainty(), Certainty::Definite);
        }
        Ok(_) => return Err("expected conflict"),
    }
    assert_eq!(script.seen.len(), 1);
    assert_ne!(script.seen[0].method, Method::Delete);
    assert!(!format!("{:?}", script.seen[0]).contains("other-session"));
    Ok(())
}

#[test]
fn refresh_patches_the_same_session() -> Result<(), &'static str> {
    let mut script = Script::once(200, BODY);
    let refreshed =
        refresh_session(&mut script, 7, "sess", "admin-canary").map_err(|_| "refresh")?;
    assert_eq!(refreshed.token(), "queue-token-canary");
    assert_eq!(script.seen[0].method, Method::Patch);
    assert!(script.seen[0].path.ends_with("/7/sessions/sess"));
    assert_eq!(script.seen[0].body.len(), 0);
    let rejected = create_session(&mut Script::once(200, BODY), 7, "", "admin-canary");
    assert_eq!(
        rejected.map(|_| ()),
        Err(SessionError::Wire(WireError::RegistrationRejected))
    );
    Ok(())
}

#[test]
fn refresh_skips_when_the_snapshot_already_moved() -> Result<(), &'static str> {
    let mut created = Script::once(200, BODY);
    let held =
        create_session(&mut created, 7, "velnor-host", "admin-canary").map_err(|_| "held")?;
    let mut skipped_transport = Script::once(200, BODY);
    let skipped = refresh_if_current(
        &mut skipped_transport,
        7,
        &held,
        "other",
        held.token(),
        "admin-canary",
    )
    .map_err(|_| "skip")?;
    if skipped.is_some() || !skipped_transport.seen.is_empty() {
        return Err("stale snapshot called the transport");
    }
    let mut matched = Script::once(200, BODY);
    let again = refresh_if_current(
        &mut matched,
        7,
        &held,
        &held.session_id,
        held.token(),
        "admin-canary",
    )
    .map_err(|_| "match")?;
    let Some(again) = again else {
        return Err("matching snapshot did not refresh");
    };
    assert_eq!(again.session_id, "sess");
    assert_eq!(matched.seen.len(), 1);
    assert_eq!(matched.seen[0].method, Method::Patch);
    Ok(())
}
