use super::super::{VerifiedPoolSessionAdmin, VerifiedQueueSession};
use super::{AckState, acknowledgement_allowed};
use crate::policy::{PollWithTrust, PoolBinding, PoolRegistrationScope};
use crate::{
    Ack, AdminConnectionCall, DiscoveryTransport, Exchange, InnerJob, InnerKind, Method,
    ParsedBatch, RefreshGate, SessionError, SessionRequest, Transport, TransportFail,
    admin_connection_once,
};
use std::collections::{BTreeSet, VecDeque};
use std::time::{Duration, SystemTime};

const ADMIN_RESPONSE: &str =
    r#"{"url":"https://pipelinesghubeus9.actions.githubusercontent.com","token":"admin-canary"}"#;
const SESSION_RESPONSE: &str = r#"{"sessionId":"session-1","messageQueueUrl":"https://queue.example/_apis/runtime/runnerscalesets/7/sessions/session-1/messages?tenant=private%2Fid&lastMessageId=old&lastMessageId=duplicate","messageQueueAccessToken":"queue-canary"}"#;
const REFRESHED_SESSION_RESPONSE: &str = r#"{"sessionId":"session-1","messageQueueUrl":"https://queue-new.example/_apis/runtime/runnerscalesets/7/sessions/session-1/rotated/messages?tenant=private%2Fid&lastMessageId=refresh","messageQueueAccessToken":"replacement-queue-canary"}"#;
const SESSION_ID: i64 = 7;
const MESSAGE_PATH: &str = "/_apis/runtime/runnerscalesets/7/sessions/session-1/messages";
const REFRESHED_MESSAGE_PATH: &str =
    "/_apis/runtime/runnerscalesets/7/sessions/session-1/rotated/messages";

fn available_batch() -> ParsedBatch {
    ParsedBatch {
        message_id: 17,
        statistics: None,
        jobs: vec![InnerJob {
            kind: InnerKind::Available,
            request_id: Some(23),
            job_id: Some("opaque-job".to_owned()),
            workflow_run_id: Some(31),
            owner_name: Some("ChainArgos".to_owned()),
            repository_name: Some("java-monorepo".to_owned()),
            event_name: Some("push".to_owned()),
            labels: Vec::new(),
            runner_id: None,
            runner_name: None,
            result: None,
            fields: Vec::new(),
        }],
    }
}

fn empty_ids() -> BTreeSet<i64> {
    BTreeSet::new()
}

struct Script {
    replies: VecDeque<Result<Exchange, TransportFail>>,
    seen: Vec<SessionRequest>,
    origin: String,
    origins: Vec<String>,
}

impl Script {
    fn with_replies(poll_body: &str, followup: Vec<Result<Exchange, TransportFail>>) -> Self {
        let mut poll_replies = vec![Ok(exchange(200, poll_body))];
        poll_replies.extend(followup);
        Self::with_session_responses(SESSION_RESPONSE, poll_replies)
    }

    fn with_session_responses(
        session_response: &str,
        poll_replies: Vec<Result<Exchange, TransportFail>>,
    ) -> Self {
        let mut replies = VecDeque::from([
            Ok(exchange(201, ADMIN_RESPONSE)),
            Ok(exchange(200, session_response)),
        ]);
        replies.extend(poll_replies);
        Self {
            replies,
            seen: Vec::new(),
            origin: "https://queue.example".to_owned(),
            origins: Vec::new(),
        }
    }
}

impl Transport for Script {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.seen.push(request.clone());
        self.origins.push(self.origin.clone());
        self.replies.pop_front().ok_or(TransportFail::Reset)?
    }
}

impl DiscoveryTransport for Script {
    fn bind_github_api_origin(&mut self) -> Result<(), SessionError> {
        Ok(())
    }

    fn bind_actions_service_origin(&mut self, url: &str) -> Result<(), SessionError> {
        let authority = url
            .strip_prefix("https://")
            .and_then(|rest| rest.split('/').next())
            .filter(|authority| !authority.is_empty() && !authority.contains('@'))
            .ok_or(SessionError::Uncertain)?;
        self.origin = format!("https://{authority}");
        Ok(())
    }

    fn bind_message_queue_origin(
        &mut self,
        url: &str,
    ) -> Result<crate::MessageQueueRoute, SessionError> {
        let rest = url
            .strip_prefix("https://")
            .ok_or(SessionError::Uncertain)?;
        let (authority, suffix) = rest.split_once('/').unwrap_or((rest, ""));
        if authority.is_empty() || authority.contains('@') || authority.contains('?') {
            return Err(SessionError::Uncertain);
        }
        self.origin = format!("https://{authority}");
        let (path, query) = suffix.split_once('?').unwrap_or((suffix, ""));
        crate::MessageQueueRoute::from_parts(
            if path.is_empty() {
                "/".to_owned()
            } else {
                format!("/{path}")
            },
            (!query.is_empty()).then(|| query.to_owned()),
        )
        .map_err(SessionError::from)
    }
}

fn exchange(status: u16, body: &str) -> Exchange {
    Exchange {
        status,
        body: body.as_bytes().to_vec(),
    }
}

// This direct fixture exercises queue-operation state transitions only.
// Production capabilities are created exclusively by the proof-gated
// pool preflight; it does not fabricate a verified image or policy proof.
fn test_capability_and_session(
    poll_body: &str,
    ack: Result<Exchange, TransportFail>,
) -> Result<(Script, VerifiedPoolSessionAdmin, VerifiedQueueSession, i64), &'static str> {
    test_capability_and_session_with_replies(poll_body, vec![ack])
}

fn test_capability_and_session_with_replies(
    poll_body: &str,
    followup: Vec<Result<Exchange, TransportFail>>,
) -> Result<(Script, VerifiedPoolSessionAdmin, VerifiedQueueSession, i64), &'static str> {
    let mut script = Script::with_replies(poll_body, followup);
    let connection = admin_connection_once(
        &mut script,
        &AdminConnectionCall {
            config_url: "https://github.com/ChainArgos/java-monorepo",
            registration_token: "registration-canary",
        },
    )
    .map_err(|_| "admin connection")?;
    let binding = PoolBinding {
        registration_scope: PoolRegistrationScope::Repository {
            owner: "ChainArgos".to_owned(),
            repository: "java-monorepo".to_owned(),
        },
        repository_id: 829_618_808,
        repository_full_name: "ChainArgos/java-monorepo".to_owned(),
        scale_set_id: SESSION_ID,
        scale_set_name: "synthetic-session-test-set".to_owned(),
        actions_runner_group_id: 1,
        actions_runner_group_name: "Default".to_owned(),
        rest_runner_group_id: Some(1),
        runner_image_profile: None,
        runner_image: None,
        policy_digest: "policy-digest".to_owned(),
    };
    let mut capability = VerifiedPoolSessionAdmin {
        connection,
        binding,
        policy_digest: "policy-digest".to_owned(),
        session_creation_attempted: false,
        created_session_id: None,
        close_attempted: false,
        expires_at: SystemTime::now() + Duration::from_secs(300),
    };
    let mut session = capability
        .create_session(&mut script, "velnor-host")
        .map_err(|_| "create session")?;
    let polled = capability
        .poll_with_trust(&mut script, &mut session, 0, 1, &RefreshGate::new())
        .map_err(|_| "poll")?;
    let PollWithTrust::Batch(batch) = polled else {
        return Err("poll batch");
    };
    let message_id = batch.message_id();
    Ok((script, capability, session, message_id))
}

const STARTED_MESSAGE: &str = r#"{"messageId":17,"messageType":"RunnerScaleSetJobMessages","body":"[{\"messageType\":\"JobStarted\",\"runnerId\":5,\"runnerName\":\"runner-a\",\"workflowRunId\":31}]"}"#;
const AVAILABLE_MESSAGE: &str = r#"{"messageId":18,"messageType":"RunnerScaleSetJobMessages","body":"[{\"messageType\":\"JobAvailable\",\"runnerRequestId\":23}]"}"#;
const STARTED_WITH_STATS: &str = r#"{"messageId":19,"messageType":"RunnerScaleSetJobMessages","body":"[{\"messageType\":\"JobStarted\",\"runnerId\":5,\"runnerName\":\"runner-a\",\"workflowRunId\":31}]","statistics":{"totalAvailableJobs":0,"totalAcquiredJobs":0,"totalAssignedJobs":0,"totalRunningJobs":0,"totalRegisteredRunners":4,"totalBusyRunners":0,"totalIdleRunners":4}}"#;

#[test]
fn session_capability_acknowledges_only_the_latest_resolved_message_once()
-> Result<(), &'static str> {
    let (mut script, capability, mut session, message_id) =
        test_capability_and_session(STARTED_MESSAGE, Ok(exchange(204, "")))?;
    assert_eq!(message_id, 17);
    assert_eq!(script.seen.len(), 3);
    assert_eq!(script.seen[2].path, MESSAGE_PATH);
    assert_eq!(
        script.seen[2].query.as_deref(),
        Some("tenant=private%2Fid&lastMessageId=old&lastMessageId=duplicate")
    );

    let result = capability
        .acknowledge_resolved_message(
            &mut script,
            &mut session,
            message_id,
            true,
            &RefreshGate::new(),
        )
        .map_err(|_| "ack")?;
    assert_eq!(result, Ack::Deleted);
    assert_eq!(script.seen.len(), 4);
    assert_eq!(script.seen[3].method, Method::Delete);
    assert_eq!(script.seen[3].path, format!("{MESSAGE_PATH}/{message_id}"));
    assert_eq!(
        script.seen[3]
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
            .map(|(_, value)| value.as_str()),
        Some("Bearer queue-canary")
    );
    assert_eq!(session.last_message_id(), None);

    assert!(
        capability
            .acknowledge_resolved_message(
                &mut script,
                &mut session,
                message_id,
                true,
                &RefreshGate::new(),
            )
            .is_err()
    );
    assert_eq!(script.seen.len(), 4);
    Ok(())
}

#[test]
fn unresolved_available_offer_keeps_message_held_without_transport() -> Result<(), &'static str> {
    let (mut script, capability, mut session, message_id) =
        test_capability_and_session(AVAILABLE_MESSAGE, Ok(exchange(204, "")))?;
    assert_eq!(message_id, 18);
    assert_eq!(session.last_message_id(), Some(message_id));
    let requests_before_ack = script.seen.len();

    let result = capability
        .acknowledge_resolved_message(
            &mut script,
            &mut session,
            message_id,
            true,
            &RefreshGate::new(),
        )
        .map_err(|_| "suppressed ack")?;
    assert_eq!(result, Ack::Suppressed);
    assert_eq!(script.seen.len(), requests_before_ack);

    assert!(
        capability
            .poll_with_trust(
                &mut script,
                &mut session,
                message_id,
                1,
                &RefreshGate::new(),
            )
            .is_err()
    );
    assert_eq!(script.seen.len(), requests_before_ack);
    Ok(())
}

#[test]
fn uncertain_ack_is_not_replayed_or_followed_by_a_poll() -> Result<(), &'static str> {
    let (mut script, capability, mut session, message_id) =
        test_capability_and_session(STARTED_MESSAGE, Err(TransportFail::Reset))?;
    assert_eq!(
        capability.acknowledge_resolved_message(
            &mut script,
            &mut session,
            message_id,
            true,
            &RefreshGate::new(),
        ),
        Err(SessionError::Uncertain)
    );
    assert_eq!(script.seen.len(), 4);
    assert!(
        capability
            .acknowledge_resolved_message(
                &mut script,
                &mut session,
                message_id,
                true,
                &RefreshGate::new(),
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
            )
            .is_err()
    );
    assert_eq!(script.seen.len(), 4);
    Ok(())
}

#[test]
fn population_observation_is_bound_to_session_set_and_exact_poll_message()
-> Result<(), &'static str> {
    let (mut script, capability, mut session, message_id) =
        test_capability_and_session(STARTED_WITH_STATS, Ok(exchange(204, "")))?;
    let observation = session
        .population_observation()
        .ok_or("population observation")?;
    assert_eq!(
        observation.source(),
        super::super::types::PopulationObservationSource::PollBatch
    );
    assert_eq!(observation.session_id(), "session-1");
    assert_eq!(observation.scale_set_id(), SESSION_ID);
    assert_eq!(observation.message_id(), Some(message_id));
    assert_eq!(message_id, 19);
    assert_eq!(observation.statistics().total_assigned_jobs, 0);
    assert_eq!(observation.statistics().total_running_jobs, 0);
    assert_eq!(observation.statistics().total_registered_runners, 4);
    assert_eq!(
        capability
            .acknowledge_resolved_message(
                &mut script,
                &mut session,
                message_id,
                true,
                &RefreshGate::new(),
            )
            .map_err(|_| "ack after observation")?,
        Ack::Deleted
    );
    assert!(session.population_observation().is_none());
    Ok(())
}

mod population;
mod refresh;
mod validation;
