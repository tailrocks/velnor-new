// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use rsa::pkcs1::EncodeRsaPrivateKey;
use rsa::{Generate, PrivateKey};
use serde_json::{Value, json};

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

#[derive(Clone, Copy, Debug, Default)]
pub(crate) enum Renew {
    #[default]
    Renewed,
    JobGone,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Behavior {
    pub(crate) renew: Renew,
    pub(crate) cancel_after: Option<Duration>,
}

pub(crate) struct MockGithub {
    pub(crate) server: TestServer,
    jit: String,
}

fn der_read(input: &[u8]) -> Result<(u8, &[u8], &[u8])> {
    let (&tag, rest) = input.split_first().context("DER tag")?;
    let (&first, rest) = rest.split_first().context("DER length")?;
    let (length, rest) = if first & 0x80 == 0 {
        (usize::from(first), rest)
    } else {
        let count = usize::from(first & 0x7f);
        if rest.len() < count {
            bail!("DER length is cut short");
        }
        let (digits, rest) = rest.split_at(count);
        (
            digits
                .iter()
                .fold(0_usize, |length, byte| (length << 8) | usize::from(*byte)),
            rest,
        )
    };
    if rest.len() < length {
        bail!("DER content is cut short");
    }
    let (content, rest) = rest.split_at(length);
    Ok((tag, content, rest))
}

fn rsa_parameters() -> Result<Value> {
    let key = PrivateKey::generate();
    let document = key.to_pkcs1_der().context("encode the test key")?;
    let (_, mut rest, _) = der_read(document.as_bytes())?;
    let mut integers = Vec::new();
    while !rest.is_empty() {
        let (_, content, remainder) = der_read(rest)?;
        integers.push(STANDARD.encode(content));
        rest = remainder;
    }
    let [_version, modulus, exponent, d, p, q, dp, dq, inverse_q] = integers
        .try_into()
        .map_err(|_| anyhow::anyhow!("the test key is not a PKCS#1 key"))?;
    Ok(json!({
        "modulus": modulus,
        "exponent": exponent,
        "d": d,
        "p": p,
        "q": q,
        "dp": dp,
        "dq": dq,
        "inverseQ": inverse_q,
    }))
}

fn jit_config(base: &str) -> Result<String> {
    let files = json!({
        ".runner": json!({
            "AgentId": "42",
            "AgentName": "velnor-test",
            "PoolId": "1",
            "Ephemeral": "True",
            "DisableUpdate": "True",
            "ServerUrlV2": format!("{base}/broker"),
            "UseV2Flow": "true",
        }),
        ".credentials": json!({
            "Scheme": "OAuth",
            "Data": {
                "ClientId": "client-1",
                "AuthorizationUrl": format!("{base}/oauth/token"),
            },
        }),
        ".credentials_rsaparams": rsa_parameters()?,
    });
    let encoded: serde_json::Map<String, Value> = files
        .as_object()
        .context("JIT files")?
        .iter()
        .map(|(name, content)| {
            (
                name.clone(),
                Value::String(STANDARD.encode(content.to_string())),
            )
        })
        .collect();
    Ok(STANDARD.encode(Value::Object(encoded).to_string()))
}

fn message(message_type: &str, body: &Value) -> Value {
    json!({"messageId": 1, "messageType": message_type, "body": body.to_string()})
}

fn job_message(base: &str, mut job: Value) -> Result<Value> {
    let object = job.as_object_mut().context("job message is an object")?;
    object.insert("Plan".to_owned(), json!({"PlanId": PLAN_ID}));
    object.insert("BillingOwnerId".to_owned(), json!("owner-1"));
    object.insert(
        "Variables".to_owned(),
        json!({"system.github.results_endpoint": {"Value": format!("{base}/results")}}),
    );
    object.insert(
        "Resources".to_owned(),
        json!({"Endpoints": [{
            "Name": "SystemVssConnection",
            "Url": format!("{base}/run/"),
            "Authorization": {"Scheme": "OAuth", "Parameters": {"AccessToken": JOB_TOKEN}},
        }]}),
    );
    Ok(job)
}

impl MockGithub {
    pub(crate) async fn start(job: Value, behavior: Behavior) -> Result<Self> {
        let server = TestServer::start().await?;
        let base = server.uri();
        let jit = jit_config(&base)?;
        mount_identity(&server)?;
        mount_broker(&server, &base, behavior)?;
        mount_run_service(&server, job_message(&base, job)?, behavior)?;
        mount_results(&server, &base)?;
        Ok(Self { server, jit })
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
}

fn mount_identity(server: &TestServer) -> Result<()> {
    server.respond("POST", "/oauth/token", |_| {
        Reply::json(
            200,
            &json!({"access_token": OAUTH_TOKEN, "token_type": "bearer", "expires_in": 3600}),
        )
    })
}

fn mount_broker(server: &TestServer, base: &str, behavior: Behavior) -> Result<()> {
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
    let request = message(
        "RunnerJobRequest",
        &json!({
            "id": "message-1",
            "runner_request_id": "request-1",
            "should_acknowledge": true,
            "run_service_url": format!("{base}/run/"),
            "billing_owner_id": "owner-1",
        }),
    );
    let cancel = message(
        "JobCancellation",
        &json!({"JobId": JOB_ID, "Timeout": "00:01:00"}),
    );
    let delivered = Arc::new(AtomicBool::new(false));
    server.respond(
        "GET",
        "/broker/message",
        move |request_seen| match request_seen.query("status") {
            Some("Online") => Reply::json(200, &request),
            Some("Busy") => match behavior.cancel_after {
                Some(after) if !delivered.swap(true, Ordering::SeqCst) => {
                    Reply::json(200, &cancel).after(after)
                }
                _ => Reply::status(204).after(Duration::from_secs(30)),
            },
            _ => Reply::status(400),
        },
    )
}

fn mount_run_service(server: &TestServer, job: Value, behavior: Behavior) -> Result<()> {
    server.respond("POST", "/run/acquirejob", move |_| Reply::json(200, &job))?;
    server.respond("POST", "/run/renewjob", move |_| match behavior.renew {
        Renew::Renewed => Reply::json(200, &json!({"lockedUntil": "2099-01-01T00:00:00Z"})),
        Renew::JobGone => Reply::json(
            404,
            &json!({"source": "actions-run-service", "statusCode": 404, "message": "gone"}),
        ),
    })?;
    server.respond("POST", "/run/completejob", |_| Reply::status(200))
}

fn mount_results(server: &TestServer, base: &str) -> Result<()> {
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
        server.respond("PUT", blob, |_| Reply::status(201))?;
    }
    for method in ["CreateStepLogsMetadata", "CreateJobLogsMetadata"] {
        server.respond("POST", &format!("{receiver}/{method}"), |_| {
            Reply::json(200, &json!({"ok": true}))
        })?;
    }
    Ok(())
}
