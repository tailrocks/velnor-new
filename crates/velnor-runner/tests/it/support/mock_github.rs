// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use velnor_runner::protocol::SessionRecovery;

use super::jit::jit_config;
use super::mock_broker::{is_stale, mount_broker, mount_identity};
use super::mock_feed::MockFeed;
use super::mock_results::mount_results;
use super::{RecordedRequest, Reply, TestServer};

pub(crate) const OAUTH_TOKEN: &str = "oauth-token-1";
pub(crate) const STALE_OAUTH_TOKEN: &str = "oauth-token-stale";
pub(crate) const JOB_TOKEN: &str = "job-token-1";
pub(crate) const SESSION_ID: &str = "session-1";
pub(crate) const RECREATED_SESSION_ID: &str = "session-2";
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

pub(crate) fn step_id(number: usize) -> String {
    format!("10000000-0000-0000-0000-{number:012}")
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) enum Renew {
    #[default]
    Renewed,
    RenewedAfter(Duration),
    ExpiringThenFailing,
    RenewedThenJobGone,
    JobGone,
    NotFoundWithoutBody,
    ServerError,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) enum Online {
    #[default]
    Job,
    JobForFreshTokenOnly,
    ForceTokenRefreshThenJob,
    Idle,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) enum Cancel {
    #[default]
    Never,
    After(Duration),
    AfterACancelOfAnotherJob(Duration),
    HostedShutdownAfter(Duration),
    BusyPollFails(u16),
    BusyPollRunnerGone,
    BusyPollSessionExpiresOnce,
    AfterAForcedTokenRefresh(Duration),
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) enum OAuthTokens {
    #[default]
    Fresh,
    StaleFirst,
    StaleThenFails(u16),
    HangsUpAfterSignIn,
    UnavailableAfterSignIn(usize),
    UnavailableFirst(usize),
    Fails {
        status: u16,
        after: usize,
        times: usize,
        retry_after: Option<&'static str>,
    },
    Rejected(&'static str, &'static str),
    RejectedAfterSignIn(&'static str, &'static str),
    NoAccessToken,
    UnparsableBadRequestFirst(usize),
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) enum StepStatus {
    #[default]
    Accepted,
    Stalls,
    StallsFirst,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum SessionFault {
    Unavailable,
    Unparsable,
    NoSessionId,
    Forbidden,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) enum SessionCreate {
    #[default]
    Created,
    ConflictsFirst(usize),
    ConflictsAlways,
    Answers(&'static [u16]),
    Fails {
        fault: SessionFault,
        after: usize,
        times: usize,
    },
}

impl SessionCreate {
    pub(crate) const fn failing(fault: SessionFault, times: usize) -> Self {
        Self::Fails {
            fault,
            after: 0,
            times,
        }
    }

    pub(crate) const fn failing_when_recreated(fault: SessionFault, times: usize) -> Self {
        Self::Fails {
            fault,
            after: 1,
            times,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) enum Acknowledge {
    #[default]
    Accepted,
    JobGone,
    RunnerGone,
    Unavailable,
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
    pub(crate) acknowledge: Acknowledge,
    pub(crate) cancel_timeout: Duration,
    pub(crate) cancel_timeout_text: Option<&'static str>,
    pub(crate) unusable_job: bool,
    pub(crate) blob_status: u16,
    pub(crate) seal_status: Option<u16>,
    pub(crate) blob_delay: Duration,
    pub(crate) feed: bool,
    pub(crate) session: SessionCreate,
    pub(crate) recovery: SessionRecovery,
    pub(crate) oauth: OAuthTokens,
    pub(crate) acquire_delay: Duration,
    pub(crate) complete_delay: Duration,
    pub(crate) status: StepStatus,
}

impl Default for Behavior {
    fn default() -> Self {
        Self {
            renew: Renew::Renewed,
            cancel: Cancel::Never,
            online: Online::Job,
            message_types: MessageTypes::Canonical,
            should_acknowledge: true,
            acknowledge: Acknowledge::Accepted,
            cancel_timeout: Duration::from_secs(300),
            cancel_timeout_text: None,
            unusable_job: false,
            blob_status: 201,
            seal_status: None,
            blob_delay: Duration::ZERO,
            feed: false,
            session: SessionCreate::Created,
            recovery: SessionRecovery::Recreate,
            oauth: OAuthTokens::Fresh,
            acquire_delay: Duration::ZERO,
            complete_delay: Duration::ZERO,
            status: StepStatus::Accepted,
        }
    }
}

pub(crate) struct MockGithub {
    pub(crate) server: TestServer,
    pub(crate) feed: Option<MockFeed>,
    jit: String,
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
        let jit = jit_config(&base, behavior.recovery)?;
        let feed = if behavior.feed {
            Some(MockFeed::start().await?)
        } else {
            None
        };
        mount_identity(&server, behavior.oauth)?;
        mount_broker(&server, &base, &behavior)?;
        mount_run_service(&server, job_message(&base, job, feed.as_ref())?, &behavior)?;
        mount_results(&server, &base, &behavior)?;
        Ok(Self { server, feed, jit })
    }

    pub(crate) fn jit(&self) -> &str {
        &self.jit
    }

    pub(crate) fn requests_to(&self, method: &str, path: &str) -> Result<Vec<RecordedRequest>> {
        self.server.requests_to(method, path)
    }

    pub(crate) fn uploaded(&self, blob: &str) -> Result<String> {
        let puts = self.requests_to("PUT", blob)?;
        anyhow::ensure!(!puts.is_empty(), "{blob} was uploaded");
        Ok(puts.iter().map(RecordedRequest::text).collect())
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

fn mount_run_service(server: &TestServer, job: Value, behavior: &Behavior) -> Result<()> {
    let (unusable_job, renew) = (behavior.unusable_job, behavior.renew);
    let acquire_delay = behavior.acquire_delay;
    server.respond("POST", "/run/acquirejob", move |seen| {
        if is_stale(seen) {
            Reply::status(401)
        } else if unusable_job {
            Reply::json(200, &json!({"unexpected": "not a job message"}))
        } else {
            Reply::json(200, &job).after(acquire_delay)
        }
    })?;
    let renewals = AtomicUsize::new(0);
    server.respond("POST", "/run/renewjob", move |_| match renew {
        Renew::Renewed => Reply::json(200, &json!({"lockedUntil": "2099-01-01T00:00:00Z"})),
        Renew::RenewedAfter(delay) => {
            Reply::json(200, &json!({"lockedUntil": "2099-01-01T00:00:00Z"})).after(delay)
        }
        Renew::RenewedThenJobGone if renewals.fetch_add(1, Ordering::SeqCst) == 0 => {
            Reply::json(200, &json!({"lockedUntil": "2099-01-01T00:00:00Z"}))
        }
        Renew::ExpiringThenFailing if renewals.fetch_add(1, Ordering::SeqCst) == 0 => {
            let expires = OffsetDateTime::now_utc() + Duration::from_secs(1);
            Reply::json(
                200,
                &json!({"lockedUntil": expires.format(&Rfc3339).unwrap_or_default()}),
            )
        }

        Renew::JobGone | Renew::RenewedThenJobGone => Reply::json(
            404,
            &json!({"source": "actions-run-service", "statusCode": 404, "message": "gone"}),
        ),
        Renew::NotFoundWithoutBody => Reply::status(404),
        Renew::ExpiringThenFailing | Renew::ServerError => {
            Reply::json(500, &json!({"message": "unavailable"}))
        }
    })?;
    let complete_delay = behavior.complete_delay;
    server.respond("POST", "/run/completejob", move |_| {
        Reply::status(200).after(complete_delay)
    })
}
