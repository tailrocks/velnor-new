// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::{Value, json};

use super::jit::jit_config;
use super::mock_feed::MockFeed;
use super::{RecordedRequest, Reply, TestServer};

pub(crate) const OAUTH_TOKEN: &str = "oauth-token-1";
pub(crate) const JOB_TOKEN: &str = "job-token-1";
pub(crate) const SESSION_ID: &str = "session-1";
pub(crate) const PLAN_ID: &str = "00000000-0000-0000-0000-000000000002";
pub(crate) const JOB_ID: &str = "00000000-0000-0000-0000-000000000001";
pub(crate) const RESULTS_STEPS_UPDATE: &str =
    "/results/twirp/github.actions.results.api.v1.WorkflowStepUpdateService/WorkflowStepsUpdate";
pub(crate) const STEP_LOG_BLOB: &str = "/blob/step-log";
pub(crate) const JOB_LOG_BLOB: &str = "/blob/job-log";
pub(crate) const SUMMARY_BLOB: &str = "/blob/summary";

pub(crate) const STEP_LOG_URL: &str =
    "/results/twirp/results.services.receiver.Receiver/GetStepLogsSignedBlobURL";
pub(crate) const OTHER_JOB_ID: &str = "00000000-0000-0000-0000-0000000000ff";
const POLL_STALL: Duration = Duration::from_secs(30);

pub(crate) fn step_id(number: usize) -> String {
    format!("10000000-0000-0000-0000-{number:012}")
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) enum Renew {
    #[default]
    Renewed,
    JobGone,
    ServerError,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) enum Online {
    #[default]
    Job,
    JobAfterOneUnauthorized,
    Idle,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) enum Cancel {
    #[default]
    Never,
    After(Duration),
    AfterACancelOfAnotherJob(Duration),
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) enum MessageTypes {
    #[default]
    Canonical,
    Lowercase,
}

#[derive(Clone, Debug)]
pub(crate) struct Behavior {
    pub(crate) renew: Renew,
    pub(crate) cancel: Cancel,
    pub(crate) online: Online,
    pub(crate) message_types: MessageTypes,
    pub(crate) should_acknowledge: bool,
    pub(crate) unusable_job: bool,
    pub(crate) blob_status: u16,
    pub(crate) feed: bool,
}

impl Default for Behavior {
    fn default() -> Self {
        Self {
            renew: Renew::Renewed,
            cancel: Cancel::Never,
            online: Online::Job,
            message_types: MessageTypes::Canonical,
            should_acknowledge: true,
            unusable_job: false,
            blob_status: 201,
            feed: false,
        }
    }
}

pub(crate) struct MockGithub {
    pub(crate) server: TestServer,
    pub(crate) feed: Option<MockFeed>,
    jit: String,
}

fn message(message_type: &str, body: &Value) -> Value {
    json!({"messageId": 1, "messageType": message_type, "body": body.to_string()})
}

fn job_message(base: &str, mut job: Value, feed: Option<&MockFeed>) -> Result<Value> {
    let object = job.as_object_mut().context("job message is an object")?;
    object.insert("Plan".to_owned(), json!({"PlanId": PLAN_ID}));
    object.insert("BillingOwnerId".to_owned(), json!("owner-1"));
    if let Some(steps) = object.get_mut("Steps").and_then(Value::as_array_mut) {
        for (index, step) in steps.iter_mut().enumerate() {
            step["Id"] = json!(step_id(index + 1));
        }
    }
    let variables = object
        .entry("Variables")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .context("job variables are an object")?;
    variables.insert(
        "system.github.results_endpoint".to_owned(),
        json!({"Value": format!("{base}/results")}),
    );
    let data = feed.map_or_else(|| json!({}), |feed| json!({"FeedStreamUrl": feed.url()}));
    object.insert(
        "Resources".to_owned(),
        json!({"Endpoints": [{
            "Name": "SystemVssConnection",
            "Url": format!("{base}/run/"),
            "Authorization": {"Scheme": "OAuth", "Parameters": {"AccessToken": JOB_TOKEN}},
            "Data": data,
        }]}),
    );
    Ok(job)
}

impl MockGithub {
    pub(crate) async fn start(job: Value, behavior: Behavior) -> Result<Self> {
        let server = TestServer::start().await?;
        let base = server.uri();
        let jit = jit_config(&base)?;
        let feed = if behavior.feed {
            Some(MockFeed::start().await?)
        } else {
            None
        };
        mount_identity(&server)?;
        mount_broker(&server, &base, &behavior)?;
        mount_run_service(&server, job_message(&base, job, feed.as_ref())?, &behavior)?;
        mount_results(&server, &base, behavior.blob_status)?;
        Ok(Self { server, feed, jit })
    }

    pub(crate) fn jit(&self) -> &str {
        &self.jit
    }

    pub(crate) fn requests_to(&self, method: &str, path: &str) -> Result<Vec<RecordedRequest>> {
        self.server.requests_to(method, path)
    }

    pub(crate) fn complete_request(&self) -> Result<Value> {
        self.server.only_request("POST", "/run/completejob")?.json()
    }

    pub(crate) fn polls(&self, status: &str) -> Result<usize> {
        Ok(self
            .requests_to("GET", "/broker/message")?
            .iter()
            .filter(|poll| poll.query("status") == Some(status))
            .count())
    }
}

fn mount_identity(server: &TestServer) -> Result<()> {
    server.respond("POST", "/oauth/token", |_| {
        Reply::json(
            200,
            &json!({"access_token": OAUTH_TOKEN, "token_type": "bearer", "expires_in": 3600}),
        )
    })
}

fn mount_broker(server: &TestServer, base: &str, behavior: &Behavior) -> Result<()> {
    server.respond("POST", "/broker/session", |_| {
        Reply::json(
            200,
            &json!({
                "sessionId": SESSION_ID,
                "ownerName": "host (PID: 1)",
                "agent": {"id": 42, "name": "velnor-test"},
            }),
        )
    })?;
    server.respond("DELETE", "/broker/session", |_| Reply::status(204))?;
    server.respond("POST", "/broker/acknowledge", |_| Reply::status(200))?;
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
    let cancel_of = |job_id: &str| {
        message(
            &kind("JobCancellation"),
            &json!({"JobId": job_id, "Timeout": "00:01:00"}),
        )
    };
    let (cancel, decoy) = (cancel_of(JOB_ID), cancel_of(OTHER_JOB_ID));
    let (cancel_after, decoy_first) = match behavior.cancel {
        Cancel::Never => (None, false),
        Cancel::After(after) => (Some(after), false),
        Cancel::AfterACancelOfAnotherJob(after) => (Some(after), true),
    };
    let online = behavior.online;
    let unauthorized = AtomicBool::new(matches!(online, Online::JobAfterOneUnauthorized));
    let delivered = AtomicBool::new(false);
    let busy_polls = Arc::new(AtomicUsize::new(0));
    server.respond("GET", "/broker/message", move |seen| {
        match seen.query("status") {
            Some("Online") if unauthorized.swap(false, Ordering::SeqCst) => Reply::status(401),
            Some("Online") if matches!(online, Online::Idle) => {
                Reply::status(204).after(POLL_STALL)
            }
            Some("Online") => Reply::json(200, &request),
            Some("Busy") => {
                let number = busy_polls.fetch_add(1, Ordering::SeqCst);
                match cancel_after {
                    _ if decoy_first && number == 0 => Reply::json(200, &decoy),
                    Some(after) if !delivered.swap(true, Ordering::SeqCst) => {
                        Reply::json(200, &cancel).after(after)
                    }
                    _ => Reply::status(204).after(POLL_STALL),
                }
            }
            _ => Reply::status(400),
        }
    })
}

fn mount_run_service(server: &TestServer, job: Value, behavior: &Behavior) -> Result<()> {
    let (unusable_job, renew) = (behavior.unusable_job, behavior.renew);
    server.respond("POST", "/run/acquirejob", move |_| {
        if unusable_job {
            Reply::json(200, &json!({"unexpected": "not a job message"}))
        } else {
            Reply::json(200, &job)
        }
    })?;
    server.respond("POST", "/run/renewjob", move |_| match renew {
        Renew::Renewed => Reply::json(200, &json!({"lockedUntil": "2099-01-01T00:00:00Z"})),
        Renew::JobGone => Reply::json(
            404,
            &json!({"source": "actions-run-service", "statusCode": 404, "message": "gone"}),
        ),
        Renew::ServerError => Reply::json(500, &json!({"message": "unavailable"})),
    })?;
    server.respond("POST", "/run/completejob", |_| Reply::status(200))
}

fn mount_results(server: &TestServer, base: &str, blob_status: u16) -> Result<()> {
    server.respond("POST", RESULTS_STEPS_UPDATE, |_| {
        Reply::json(200, &json!({"ok": true}))
    })?;
    let receiver = "/results/twirp/results.services.receiver.Receiver";
    for (method, blob) in [
        ("GetStepLogsSignedBlobURL", STEP_LOG_BLOB),
        ("GetJobLogsSignedBlobURL", JOB_LOG_BLOB),
    ] {
        let url = format!("{base}{blob}");
        server.respond("POST", &format!("{receiver}/{method}"), move |_| {
            Reply::json(200, &json!({"logs_url": url}))
        })?;
        server.respond("PUT", blob, move |_| Reply::status(blob_status))?;
    }
    let summary_url = format!("{base}{SUMMARY_BLOB}");
    server.respond(
        "POST",
        &format!("{receiver}/GetStepSummarySignedBlobURL"),
        move |_| Reply::json(200, &json!({"summary_url": summary_url})),
    )?;
    server.respond("PUT", SUMMARY_BLOB, move |_| Reply::status(blob_status))?;
    for method in [
        "CreateStepLogsMetadata",
        "CreateJobLogsMetadata",
        "CreateStepSummaryMetadata",
    ] {
        server.respond("POST", &format!("{receiver}/{method}"), |_| {
            Reply::json(200, &json!({"ok": true}))
        })?;
    }
    Ok(())
}
