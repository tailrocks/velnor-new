// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result, bail};
use velnor_runner::protocol::{
    BrokerErrorCategory, CompletionAcknowledgement, broker_error_category,
};

use super::support::{
    JOB_ID, PLAN_ID, Scripted, TestServer, assert_bearer, assert_github_status, complete_job,
    foreign_source_error, immediate_run_service, run_service_error, run_service_url, scripted,
};

const COMPLETE_PATH: &str = "/run/jobs/123/completejob";
const COMPLETE_ACTION: &str = "complete run-service job";
const UPSTREAM_ATTEMPTS: usize = 5;

enum Expected {
    Acknowledged(CompletionAcknowledgement),
    Rejected(u16, BrokerErrorCategory),
    Transport,
}

struct CompleteCase {
    name: &'static str,
    script: Vec<Scripted>,
    requests: usize,
    expected: Expected,
}

fn complete_cases() -> Vec<CompleteCase> {
    vec![
        CompleteCase {
            name: "persistent 500 stops after the fifth attempt",
            script: vec![Scripted::Text(500, "temporary")],
            requests: UPSTREAM_ATTEMPTS,
            expected: Expected::Rejected(500, BrokerErrorCategory::Transient),
        },
        CompleteCase {
            name: "500s then success is accepted on the fifth attempt",
            script: vec![
                Scripted::Text(500, "temporary"),
                Scripted::Text(500, "temporary"),
                Scripted::Text(500, "temporary"),
                Scripted::Text(500, "temporary"),
                Scripted::Text(204, ""),
            ],
            requests: UPSTREAM_ATTEMPTS,
            expected: Expected::Acknowledged(CompletionAcknowledgement::Accepted),
        },
        CompleteCase {
            name: "a 400 is retried like every other error",
            script: vec![
                Scripted::Text(400, "bad completion"),
                Scripted::Text(400, "bad completion"),
                Scripted::Text(204, ""),
            ],
            requests: 3,
            expected: Expected::Acknowledged(CompletionAcknowledgement::Accepted),
        },
        CompleteCase {
            name: "a persistent 400 exhausts the attempts",
            script: vec![Scripted::Text(400, "bad completion")],
            requests: UPSTREAM_ATTEMPTS,
            expected: Expected::Rejected(400, BrokerErrorCategory::Transient),
        },
        CompleteCase {
            name: "a 401 is not retried",
            script: vec![Scripted::Text(401, "unauthorized")],
            requests: 1,
            expected: Expected::Rejected(401, BrokerErrorCategory::Terminal),
        },
        CompleteCase {
            name: "a typed 404 means the service already observed the job as terminal",
            script: vec![Scripted::Json(404, run_service_error(404))],
            requests: 1,
            expected: Expected::Acknowledged(CompletionAcknowledgement::RemoteObservedTerminal),
        },
        CompleteCase {
            name: "a 404 from another source is retried",
            script: vec![Scripted::Json(404, foreign_source_error(404))],
            requests: UPSTREAM_ATTEMPTS,
            expected: Expected::Rejected(404, BrokerErrorCategory::Transient),
        },
        CompleteCase {
            name: "a 404 without a source is retried",
            script: vec![Scripted::Text(404, "not found")],
            requests: UPSTREAM_ATTEMPTS,
            expected: Expected::Rejected(404, BrokerErrorCategory::Transient),
        },
        CompleteCase {
            name: "a dropped connection is retried",
            script: vec![Scripted::Hangup],
            requests: UPSTREAM_ATTEMPTS,
            expected: Expected::Transport,
        },
    ]
}

#[tokio::test]
async fn complete_job_follows_the_upstream_retry_policy() -> Result<()> {
    let server = TestServer::start().await?;
    let run_service = immediate_run_service()?;

    for case in complete_cases() {
        server.reset()?;
        server.respond("POST", COMPLETE_PATH, scripted(case.script))?;

        let outcome = run_service
            .complete_job_with_acknowledgement(&run_service_url(&server), &complete_job())
            .await;

        match (case.expected, outcome) {
            (Expected::Acknowledged(expected), Ok(actual)) => {
                assert_eq!(actual, expected, "{}", case.name);
            }
            (Expected::Rejected(status, category), Err(error)) => {
                assert_github_status(&error, status, COMPLETE_ACTION)
                    .with_context(|| case.name.to_owned())?;
                assert_eq!(
                    broker_error_category(&error),
                    Some(category),
                    "{}",
                    case.name
                );
            }
            (Expected::Transport, Err(error)) => {
                let rendered = format!("{error:#}");
                assert!(
                    rendered.contains("complete run-service job request"),
                    "{}: {rendered}",
                    case.name
                );
            }
            (_, outcome) => bail!("{}: unexpected outcome {outcome:?}", case.name),
        }
        assert_eq!(
            server.requests_to("POST", COMPLETE_PATH)?.len(),
            case.requests,
            "{}",
            case.name
        );
    }
    Ok(())
}

#[tokio::test]
async fn complete_job_resends_the_identical_body_on_every_attempt() -> Result<()> {
    let server = TestServer::start().await?;
    server.respond(
        "POST",
        COMPLETE_PATH,
        scripted(vec![
            Scripted::Text(500, "temporary"),
            Scripted::Text(204, ""),
        ]),
    )?;

    immediate_run_service()?
        .complete_job(&run_service_url(&server), &complete_job())
        .await?;

    let requests = server.requests_to("POST", COMPLETE_PATH)?;
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].json()?, requests[1].json()?);
    assert_eq!(requests[1].json()?["planId"], PLAN_ID);
    assert_eq!(requests[1].json()?["jobId"], JOB_ID);
    assert_bearer(&requests[1]);
    Ok(())
}
