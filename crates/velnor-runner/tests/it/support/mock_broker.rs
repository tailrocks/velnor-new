// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use anyhow::Result;
use serde_json::{Value, json};

use super::mock_github::{
    Acknowledge, Behavior, Cancel, JOB_ID, MessageTypes, OAUTH_TOKEN, OAuthTokens, OTHER_JOB_ID,
    Online, RECREATED_SESSION_ID, SESSION_ID, STALE_OAUTH_TOKEN, SessionCreate, SessionFault,
};
use super::{Reply, TestServer};

const POLL_STALL: Duration = Duration::from_secs(30);

fn timespan(timeout: Duration) -> String {
    let seconds = timeout.as_secs();
    format!(
        "0.{:02}:{:02}:{:02}.0000",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

fn message(message_type: &str, body: &Value) -> Value {
    json!({"messageId": 1, "messageType": message_type, "body": body.to_string()})
}

pub(super) fn is_stale(request: &super::RecordedRequest) -> bool {
    request.header("authorization") == Some(format!("Bearer {STALE_OAUTH_TOKEN}").as_str())
}

pub(super) fn mount_identity(server: &TestServer, tokens: OAuthTokens) -> Result<()> {
    let exchanges = AtomicUsize::new(0);
    server.respond("POST", "/oauth/token", move |_| {
        let number = exchanges.fetch_add(1, Ordering::SeqCst);
        if let OAuthTokens::Rejected(error, description) = tokens {
            return Reply::json(400, &json!({"error": error, "error_description": description}));
        }
        if matches!(tokens, OAuthTokens::UnavailableFirst(count) if number < count) {
            return Reply::status(503);
        }
        if matches!(tokens, OAuthTokens::UnavailableAfterSignIn(count) if (1..=count).contains(&number))
        {
            return Reply::status(503);
        }
        if let OAuthTokens::Fails {
            status,
            after,
            times,
            retry_after,
        } = tokens
            && number >= after
            && number - after < times
        {
            let reply = Reply::status(status);
            return match retry_after {
                Some(value) => reply.with_header("retry-after", value),
                None => reply,
            };
        }
        let stale = matches!(tokens, OAuthTokens::StaleFirst) && number == 0;
        let token = if stale {
            STALE_OAUTH_TOKEN
        } else {
            OAUTH_TOKEN
        };
        Reply::json(
            200,
            &json!({"access_token": token, "token_type": "bearer", "expires_in": 3600}),
        )
    })
}

fn broker_error(kind: &str, message: &str) -> Reply {
    Reply::json(
        404,
        &json!({
            "source": "actions-broker-listener",
            "errorKind": kind,
            "statusCode": 404,
            "errorMessage": message,
        }),
    )
}

fn busy_failure(cancel: Cancel) -> Option<Reply> {
    match cancel {
        Cancel::BusyPollFails(status) => Some(Reply::status(status)),
        Cancel::BusyPollRunnerGone => {
            Some(broker_error("RunnerNotFound", "the runner was not found"))
        }
        _ => None,
    }
}

fn mount_acknowledge(server: &TestServer, acknowledge: Acknowledge) -> Result<()> {
    server.respond("POST", "/broker/acknowledge", move |seen| {
        if is_stale(seen) {
            return Reply::status(401);
        }
        match acknowledge {
            Acknowledge::Accepted => Reply::status(200),
            Acknowledge::JobGone => broker_error("AcknowledgeJobNotFound", "the job was withdrawn"),
            Acknowledge::RunnerGone => broker_error("RunnerNotFound", "the runner was not found"),
            Acknowledge::Unavailable => Reply::status(503),
        }
    })
}

fn mount_session(server: &TestServer, create: SessionCreate) -> Result<()> {
    let attempts = AtomicUsize::new(0);
    let created = AtomicUsize::new(0);
    server.respond("POST", "/broker/session", move |_| {
        let attempt = attempts.fetch_add(1, Ordering::SeqCst);
        let conflict = || {
            Reply::json(
                409,
                &json!({"message": "A session for this runner already exists."}),
            )
        };
        match create {
            SessionCreate::ConflictsFirst(count) if attempt < count => return conflict(),
            SessionCreate::ConflictsAlways => return conflict(),
            SessionCreate::Answers(statuses) => {
                if let Some(&status) = statuses.get(attempt) {
                    return Reply::status(status);
                }
            }
            SessionCreate::Fails {
                fault,
                after,
                times,
            } if attempt >= after && attempt - after < times => {
                return match fault {
                    SessionFault::Unavailable => Reply::status(503),
                    SessionFault::Unparsable => Reply::json(200, &json!("not a session")),
                    SessionFault::Forbidden => Reply::status(403),
                };
            }
            SessionCreate::Created
            | SessionCreate::ConflictsFirst(_)
            | SessionCreate::Fails { .. } => {}
        }
        let created = if created.fetch_add(1, Ordering::SeqCst) == 0 {
            SESSION_ID
        } else {
            RECREATED_SESSION_ID
        };
        Reply::json(
            200,
            &json!({
                "sessionId": created,
                "ownerName": "host (PID: 1)",
                "agent": {"id": 42, "name": "velnor-test"},
            }),
        )
    })?;
    server.respond("DELETE", "/broker/session", |seen| {
        Reply::status(if is_stale(seen) { 401 } else { 204 })
    })
}

pub(super) fn mount_broker(server: &TestServer, base: &str, behavior: &Behavior) -> Result<()> {
    mount_session(server, behavior.session)?;
    mount_acknowledge(server, behavior.acknowledge)?;
    let kind = |name: &str| match behavior.message_types {
        MessageTypes::Lowercase => name.to_lowercase(),
        MessageTypes::Canonical => name.to_owned(),
    };
    let request = message(
        &kind("RunnerJobRequest"),
        &json!({
            "id": "message-1",
            "runner_request_id": "request-1",
            "should_acknowledge": behavior.should_acknowledge,
            "run_service_url": format!("{base}/run/"),
            "billing_owner_id": "owner-1",
        }),
    );
    let timeout = behavior
        .cancel_timeout_text
        .map_or_else(|| timespan(behavior.cancel_timeout), str::to_owned);
    let cancel_of = |job_id: &str| {
        message(
            &kind("JobCancellation"),
            &json!({"jobId": job_id, "timeout": timeout}),
        )
    };
    let shutdown = message(
        &kind("HostedRunnerShutdown"),
        &json!({"reason": "the hosted runner was deprovisioned"}),
    );
    let (cancel, decoy) = (cancel_of(JOB_ID), cancel_of(OTHER_JOB_ID));
    let (cancel_after, decoy_first, hosted_shutdown) = match behavior.cancel {
        Cancel::Never
        | Cancel::BusyPollFails(_)
        | Cancel::BusyPollRunnerGone
        | Cancel::BusyPollSessionExpiresOnce => (None, false, false),
        Cancel::After(after) => (Some(after), false, false),
        Cancel::AfterACancelOfAnotherJob(after) => (Some(after), true, false),
        Cancel::HostedShutdownAfter(after) => (Some(after), false, true),
    };
    let busy_failure = busy_failure(behavior.cancel);
    let session_invalid = broker_error("RunnerSessionInvalid", "the session is no longer valid");
    let expires_session = matches!(behavior.cancel, Cancel::BusyPollSessionExpiresOnce);
    let online = behavior.online;
    let refresh = message(&kind("ForceTokenRefresh"), &json!({}));
    let refresh_pending = AtomicBool::new(matches!(online, Online::ForceTokenRefreshThenJob));
    let delivered = AtomicBool::new(false);
    let busy_polls = Arc::new(AtomicUsize::new(0));
    server.respond("GET", "/broker/message", move |seen| {
        match seen.query("status") {
            Some("Online") if matches!(online, Online::JobForFreshTokenOnly) && is_stale(seen) => {
                Reply::status(401)
            }
            Some("Online") if refresh_pending.swap(false, Ordering::SeqCst) => {
                Reply::json(200, &refresh)
            }
            Some("Online") if matches!(online, Online::Idle) => {
                Reply::status(204).after(POLL_STALL)
            }
            Some("Online") => Reply::json(200, &request),
            Some("Busy") => {
                let number = busy_polls.fetch_add(1, Ordering::SeqCst);
                if let Some(failure) = &busy_failure {
                    return failure.clone();
                }
                if expires_session && seen.query("sessionId") == Some(SESSION_ID) {
                    return session_invalid.clone();
                }
                match cancel_after {
                    _ if decoy_first && number == 0 => Reply::json(200, &decoy),
                    Some(after) if !delivered.swap(true, Ordering::SeqCst) => {
                        let delivery = if hosted_shutdown { &shutdown } else { &cancel };
                        Reply::json(200, delivery).after(after)
                    }
                    _ => Reply::status(204).after(POLL_STALL),
                }
            }
            _ => Reply::status(400),
        }
    })
}
