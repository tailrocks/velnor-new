use std::collections::VecDeque;
use std::time::{Duration, SystemTime};

use crate::policy::{
    JobTrustEvidence, JobTrustPolicyView, JobTrustRuleView, PollWithTrust, PoolBinding,
    PoolRegistrationScope, ReusableWorkflowRuleView, verify_job_offer,
};
use crate::{
    Ack, AdminConnectionCall, DiscoveryTransport, Exchange, Method, RefreshGate,
    SessionCloseOutcome, SessionError, SessionRequest, Transport, TransportFail,
    VerifiedAcquireOutcome, VerifiedPoolSessionAdmin, WireError, admin_connection_once,
};

const SET_ID: i64 = 7;
const QUEUE_PATH: &str = "/_apis/runtime/runnerscalesets/7/sessions/session-1/messages";
const ADMIN_BODY: &str =
    r#"{"url":"https://pipelinesghubeus9.actions.githubusercontent.com","token":"admin-canary"}"#;
const SESSION_BODY: &str = r#"{"sessionId":"session-1","messageQueueUrl":"https://queue.example/_apis/runtime/runnerscalesets/7/sessions/session-1/messages","messageQueueAccessToken":"queue-canary"}"#;
const ASSIGNED_WITH_DEMAND: &str = r#"{"messageId":19,"messageType":"RunnerScaleSetJobMessages","body":"[{\"messageType\":\"JobAssigned\",\"jobWorkflowRef\":\"ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/heads/main\"}]","statistics":{"totalAvailableJobs":0,"totalAcquiredJobs":1,"totalAssignedJobs":1,"totalRunningJobs":0,"totalRegisteredRunners":0,"totalBusyRunners":0,"totalIdleRunners":0}}"#;
const ASSIGNED_EXCESS_DEMAND: &str = r#"{"messageId":23,"messageType":"RunnerScaleSetJobMessages","body":"[{\"messageType\":\"JobAssigned\"}]","statistics":{"totalAvailableJobs":0,"totalAcquiredJobs":4,"totalAssignedJobs":5,"totalRunningJobs":1,"totalRegisteredRunners":1,"totalBusyRunners":1,"totalIdleRunners":0}}"#;
const AVAILABLE_WITH_DEMAND: &str = r#"{"messageId":20,"messageType":"RunnerScaleSetJobMessages","body":"[{\"messageType\":\"JobAvailable\",\"runnerRequestId\":23,\"jobWorkflowRef\":\"ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/heads/main\"}]","statistics":{"totalAvailableJobs":1,"totalAcquiredJobs":0,"totalAssignedJobs":1,"totalRunningJobs":0,"totalRegisteredRunners":0,"totalBusyRunners":0,"totalIdleRunners":0}}"#;
const STARTED_WITH_NO_DEMAND: &str = r#"{"messageId":21,"messageType":"RunnerScaleSetJobMessages","body":"[{\"messageType\":\"JobStarted\",\"runnerId\":5,\"runnerName\":\"runner-a\",\"workflowRunId\":31}]","statistics":{"totalAvailableJobs":0,"totalAcquiredJobs":0,"totalAssignedJobs":0,"totalRunningJobs":0,"totalRegisteredRunners":0,"totalBusyRunners":0,"totalIdleRunners":0}}"#;
const AVAILABLE_TRUSTED: &str = r#"{"messageId":22,"messageType":"RunnerScaleSetJobMessages","body":"[{\"messageType\":\"JobAvailable\",\"runnerRequestId\":23,\"jobId\":\"opaque-job\",\"workflowRunId\":31,\"ownerName\":\"ChainArgos\",\"repositoryName\":\"java-monorepo\",\"eventName\":\"push\",\"jobWorkflowRef\":\"ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/heads/main\"}]","statistics":{"totalAvailableJobs":1,"totalAcquiredJobs":0,"totalAssignedJobs":1,"totalRunningJobs":0,"totalRegisteredRunners":0,"totalBusyRunners":0,"totalIdleRunners":0}}"#;
const TRUST_RUN_BODY: &str = r#"{"id":31,"run_attempt":1,"status":"queued","event":"push","path":".github/workflows/ci.yml@main","head_sha":"0123456789abcdef0123456789abcdef01234567","head_branch":"main","head_repository":{"full_name":"ChainArgos/java-monorepo"},"referenced_workflows":[]}"#;

struct Script {
    replies: VecDeque<Result<Exchange, TransportFail>>,
    seen: Vec<SessionRequest>,
    origin: String,
    origins: Vec<String>,
}

impl Script {
    fn new(
        poll_body: &str,
        later: impl IntoIterator<Item = Result<Exchange, TransportFail>>,
    ) -> Self {
        let mut replies = VecDeque::from([
            Ok(exchange(200, ADMIN_BODY)),
            Ok(exchange(200, SESSION_BODY)),
            Ok(exchange(200, poll_body)),
        ]);
        replies.extend(later);
        Self {
            replies,
            seen: Vec::new(),
            origin: "https://initial.example".to_owned(),
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
        self.origin = "https://api.github.com".to_owned();
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
}

fn exchange(status: u16, body: &str) -> Exchange {
    Exchange {
        status,
        body: body.as_bytes().to_vec(),
    }
}

fn capability_and_polled_session(
    poll_body: &str,
    later: impl IntoIterator<Item = Result<Exchange, TransportFail>>,
) -> Result<
    (
        Script,
        VerifiedPoolSessionAdmin,
        super::VerifiedQueueSession,
        crate::policy::ParsedTrustBatch,
    ),
    &'static str,
> {
    let mut script = Script::new(poll_body, later);
    let connection = admin_connection_once(
        &mut script,
        &AdminConnectionCall {
            config_url: "https://github.com/ChainArgos/java-monorepo",
            registration_token: "registration-canary",
        },
    )
    .map_err(|_| "admin")?;
    let binding = PoolBinding {
        registration_scope: PoolRegistrationScope::Organization {
            organization: "ChainArgos".to_owned(),
        },
        repository_id: 829_618_808,
        repository_full_name: "ChainArgos/java-monorepo".to_owned(),
        scale_set_id: SET_ID,
        scale_set_name: "synthetic-session-test-set".to_owned(),
        actions_runner_group_id: 1,
        actions_runner_group_name: "Default".to_owned(),
        rest_runner_group_id: Some(1),
        runner_image_profile: None,
        runner_image: None,
        policy_digest: "synthetic-policy-digest".to_owned(),
    };
    // This test-only fixture exercises transitions and does not expose a
    // production constructor or claim a real pool policy has been verified.
    let mut capability = VerifiedPoolSessionAdmin {
        connection,
        binding,
        policy_digest: "synthetic-policy-digest".to_owned(),
        session_creation_attempted: false,
        created_session_id: None,
        close_attempted: false,
        expires_at: SystemTime::now() + Duration::from_secs(300),
    };
    // Force an unrelated origin after bootstrap to prove the capability binds
    // the admin service origin again before creating its queue session.
    script.origin = "https://stale.example".to_owned();
    let mut session = capability
        .create_session(&mut script, "velnor-test", route_created_queue)
        .map_err(|_| "create")?;
    assert_eq!(
        script.origins[1],
        "https://pipelinesghubeus9.actions.githubusercontent.com"
    );
    let polled = capability
        .poll_with_trust(
            &mut script,
            &mut session,
            0,
            1,
            &RefreshGate::new(),
            route_queue_request,
        )
        .map_err(|_| "poll")?;
    let PollWithTrust::Batch(batch) = polled else {
        return Err("expected batch");
    };
    assert_eq!(script.origins[2], "https://queue.example");
    Ok((script, capability, session, batch))
}

fn route_created_queue(transport: &mut Script, queue_url: &str) -> Result<String, SessionError> {
    let (origin, path) = queue_url
        .strip_prefix("https://")
        .and_then(|value| value.split_once('/'))
        .ok_or(SessionError::Uncertain)?;
    if origin.is_empty() || origin.contains('@') || path.is_empty() {
        return Err(SessionError::Uncertain);
    }
    transport.origin = format!("https://{origin}");
    Ok(format!("/{path}"))
}

fn route_queue_request(
    transport: &mut Script,
    queue_url: &str,
    request: &mut SessionRequest,
) -> Result<String, SessionError> {
    let path = route_created_queue(transport, queue_url)?;
    request.path.clone_from(&path);
    Ok(path)
}

fn bearer(request: &SessionRequest) -> Option<&str> {
    request
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
        .map(|(_, value)| value.as_str())
}

fn run_two_assigned_jits(
    script: &mut Script,
    capability: &VerifiedPoolSessionAdmin,
    session: &mut super::VerifiedQueueSession,
    demand: &mut super::types::VerifiedAssignedDemand,
) -> Result<(), &'static str> {
    assert_eq!(demand.scale_set_id(), SET_ID);
    assert_eq!(demand.session_id(), "session-1");
    assert_eq!(demand.message_id(), Some(23));
    assert_eq!(demand.assigned_jobs(), 5);
    assert_eq!(demand.running_jobs(), 1);
    assert_eq!(demand.remaining_jit_count(), 2);
    assert_eq!(demand.demand_id(), 1);
    assert_eq!(demand.next_slot_ordinal(), 0);
    assert_eq!(script.seen.len(), 3);

    let first = capability
        .jit_assigned_demand(script, session, demand, "runner-test")
        .map_err(|_| "assigned jit")?;
    assert_eq!(first.expose(), "encoded-test");
    assert_eq!(demand.next_slot_ordinal(), 1);
    assert_eq!(demand.remaining_jit_count(), 1);
    assert!(script.seen[3].path.ends_with("/generatejitconfig"));
    assert_eq!(bearer(&script.seen[3]), Some("Bearer admin-canary"));

    let second = capability
        .jit_assigned_demand(script, session, demand, "runner-test-2")
        .map_err(|_| "second assigned jit")?;
    assert_eq!(second.expose(), "encoded-test-2");
    assert_eq!(demand.next_slot_ordinal(), 2);
    assert_eq!(demand.remaining_jit_count(), 0);
    assert!(script.seen[4].path.ends_with("/generatejitconfig"));
    assert!(
        script
            .seen
            .iter()
            .all(|request| !request.path.contains("acquirejobs"))
    );
    Ok(())
}

#[test]
fn available_offer_is_trust_checked_acquired_then_jit_and_acknowledged() -> Result<(), &'static str>
{
    let later = [
        Ok(exchange(200, TRUST_RUN_BODY)),
        Ok(exchange(200, r#"{"count":1,"value":[23]}"#)),
        Ok(exchange(200, r#"{"encodedJITConfig":"encoded-trusted"}"#)),
        Ok(exchange(204, "")),
    ];
    let (mut script, capability, mut session, batch) =
        capability_and_polled_session(AVAILABLE_TRUSTED, later)?;
    let trust = verify_test_offer(&mut script, &batch)?;
    assert_eq!(trust.message_id(), 22);
    assert_eq!(trust.runner_request_id(), 23);
    assert_eq!(trust.source_session_id(), Some("session-1"));
    assert_eq!(trust.source_scale_set_id(), Some(SET_ID));
    assert_eq!(trust.scale_set_job_id(), Some("opaque-job"));

    let acquired = capability
        .acquire_verified(
            &mut script,
            &mut session,
            trust,
            &RefreshGate::new(),
            |_, _, _| Ok(QUEUE_PATH.to_owned()),
        )
        .map_err(|_| "acquire")?;
    let VerifiedAcquireOutcome::Acquired(acquired) = acquired else {
        return Err("exact acquire ID");
    };
    assert_eq!(script.seen[4].method, Method::Post);
    assert_eq!(
        script.origins[4],
        "https://pipelinesghubeus9.actions.githubusercontent.com"
    );
    assert!(script.seen[4].path.ends_with("/acquirejobs"));
    assert_eq!(bearer(&script.seen[4]), Some("Bearer queue-canary"));
    assert_eq!(
        capability
            .jit_verified(&mut script, &mut session, *acquired, "runner-trusted")
            .map_err(|_| "jit")?
            .expose(),
        "encoded-trusted"
    );
    assert_eq!(script.seen[5].method, Method::Post);
    assert_eq!(
        script.origins[5],
        "https://pipelinesghubeus9.actions.githubusercontent.com"
    );
    assert!(script.seen[5].path.ends_with("/generatejitconfig"));
    assert_eq!(bearer(&script.seen[5]), Some("Bearer admin-canary"));
    assert_eq!(
        capability
            .acknowledge_resolved_message(
                &mut script,
                &mut session,
                22,
                true,
                &RefreshGate::new(),
                |_, _, _| Ok(QUEUE_PATH.to_owned()),
            )
            .map_err(|_| "ack")?,
        Ack::Deleted
    );
    assert_eq!(script.seen[6].method, Method::Delete);
    assert_eq!(script.origins[6], "https://queue.example");
    assert_eq!(bearer(&script.seen[6]), Some("Bearer queue-canary"));
    assert_eq!(script.seen.len(), 7);
    Ok(())
}

fn verify_test_offer(
    script: &mut Script,
    batch: &crate::policy::ParsedTrustBatch,
) -> Result<crate::policy::VerifiedJobTrust, &'static str> {
    let run = crate::policy::get_actions_workflow_trust_run(
        script,
        "ChainArgos",
        "java-monorepo",
        31,
        "actions-read-canary",
    )
    .map_err(|_| "run metadata")?;
    let allowed_repositories = vec!["ChainArgos/java-monorepo".to_owned()];
    let allowed_events = vec!["push".to_owned()];
    let allowed_paths = vec![".github/workflows/ci.yml".to_owned()];
    let allowed_head_branches = vec!["main".to_owned()];
    let no_reusable_workflows: [ReusableWorkflowRuleView<'_>; 0] = [];
    let workflow_rules = [JobTrustRuleView {
        workflow_ref: "ChainArgos/java-monorepo/.github/workflows/ci.yml@main",
        job_workflow_ref: "ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/heads/main",
        workflow_path: ".github/workflows/ci.yml@main",
        event: "push",
        head_branch: "main",
        referenced_workflows: &no_reusable_workflows,
    }];
    let policy = JobTrustPolicyView {
        repository_full_name: "ChainArgos/java-monorepo",
        allowed_repositories: &allowed_repositories,
        allowed_events: &allowed_events,
        allowed_workflow_paths: &allowed_paths,
        allowed_head_branches: &allowed_head_branches,
        workflow_rules: &workflow_rules,
        allow_forks: false,
        policy_digest: "synthetic-policy-digest",
    };
    let JobTrustEvidence::Verified(trust) = verify_job_offer(batch, 0, &run, &policy) else {
        return Err("exact event trust");
    };
    Ok(*trust)
}

#[cfg(test)]
mod acquire_refresh;

#[cfg(test)]
mod assigned_demand;

#[cfg(test)]
mod close_tests;
