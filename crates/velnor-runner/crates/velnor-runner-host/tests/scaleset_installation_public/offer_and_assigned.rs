// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::json;
use velnor_runner_github::{
    Exchange, Method, Poll, SessionRequest, Transport, TransportFail, parse_poll,
};
use velnor_runner_host::{
    Journal, Offer, WorkflowEvent, offer, resume_deferred_offer_acquire_with_transport,
};

use super::{SOURCE_SHA, TestResult, sample_connection};

#[test]
fn public_offer_admits_assigned_batch_and_waits_on_lifecycle_only_batch() -> TestResult<()> {
    let assigned_body = serde_json::to_string(&json!([{
        "messageType": "JobAssigned",
        "runnerRequestId": 0,
        "jobId": "67c5c946-116b-52aa-9566-1d8de67a2a67"
    }]))?;
    let assigned_envelope = serde_json::to_string(&json!({
        "messageId": 3,
        "messageType": "RunnerScaleSetJobMessages",
        "body": assigned_body
    }))?;
    let assigned_poll = parse_poll(200, &assigned_envelope)?;
    let Poll::Batch(ref batch) = assigned_poll else {
        panic!("expected batch for JobAssigned");
    };
    let derived_id = batch.jobs[0].request_id.expect("derived request_id");
    assert!(derived_id > 0);
    assert_eq!(
        offer(&assigned_poll),
        Offer::Acquire {
            message_id: 3,
            ids: vec![derived_id],
        }
    );

    let lifecycle_body = serde_json::to_string(&json!([
        {
            "messageType": "JobStarted",
            "runnerRequestId": 0,
            "jobId": "67c5c946-116b-52aa-9566-1d8de67a2a67",
            "runnerId": 29,
            "runnerName": "velnor-1"
        },
        {
            "messageType": "JobCompleted",
            "runnerRequestId": 0,
            "jobId": "67c5c946-116b-52aa-9566-1d8de67a2a67",
            "runnerId": 0,
            "runnerName": "",
            "result": "succeeded"
        }
    ]))?;
    let lifecycle_envelope = serde_json::to_string(&json!({
        "messageId": 4,
        "messageType": "RunnerScaleSetJobMessages",
        "body": lifecycle_body
    }))?;
    let lifecycle_poll = parse_poll(200, &lifecycle_envelope)?;
    assert_eq!(offer(&lifecycle_poll), Offer::Wait);

    let same_batch_completed_body = serde_json::to_string(&json!([
        {
            "messageType": "JobAssigned",
            "runnerRequestId": 0,
            "jobId": "67c5c946-116b-52aa-9566-1d8de67a2a67"
        },
        {
            "messageType": "JobCompleted",
            "runnerRequestId": 0,
            "jobId": "67c5c946-116b-52aa-9566-1d8de67a2a67",
            "runnerId": 29,
            "runnerName": "velnor-1",
            "result": "succeeded"
        }
    ]))?;
    let same_batch_completed_envelope = serde_json::to_string(&json!({
        "messageId": 5,
        "messageType": "RunnerScaleSetJobMessages",
        "body": same_batch_completed_body
    }))?;
    let same_batch_completed_poll = parse_poll(200, &same_batch_completed_envelope)?;
    assert_eq!(offer(&same_batch_completed_poll), Offer::Wait);
    check_accumulated_available_and_assigned_offers(derived_id)
}

fn check_accumulated_available_and_assigned_offers(derived_id: i64) -> TestResult<()> {
    let accumulated_body = serde_json::to_string(&json!([
        {
            "messageType": "JobAvailable",
            "runnerRequestId": 0,
            "jobId": "67c5c946-116b-52aa-9566-1d8de67a2a67",
            "repositoryName": "repo"
        },
        {
            "messageType": "JobAssigned",
            "runnerRequestId": 0,
            "jobId": "67c5c946-116b-52aa-9566-1d8de67a2a67",
            "repositoryName": "repo"
        }
    ]))?;
    let accumulated_envelope = serde_json::to_string(&json!({
        "messageId": 6,
        "messageType": "RunnerScaleSetJobMessages",
        "body": accumulated_body
    }))?;
    let accumulated_poll = parse_poll(200, &accumulated_envelope)?;
    assert_eq!(
        offer(&accumulated_poll),
        Offer::Acquire {
            message_id: 6,
            ids: vec![derived_id],
        }
    );

    let mismatched_body = serde_json::to_string(&json!([
        {
            "messageType": "JobAvailable",
            "runnerRequestId": 0,
            "jobId": "67c5c946-116b-52aa-9566-1d8de67a2a67",
            "repositoryName": "repo-a"
        },
        {
            "messageType": "JobAssigned",
            "runnerRequestId": 0,
            "jobId": "67c5c946-116b-52aa-9566-1d8de67a2a67",
            "repositoryName": "repo-b"
        }
    ]))?;
    let mismatched_envelope = serde_json::to_string(&json!({
        "messageId": 7,
        "messageType": "RunnerScaleSetJobMessages",
        "body": mismatched_body
    }))?;
    let mismatched_poll = parse_poll(200, &mismatched_envelope)?;
    assert_eq!(
        offer(&mismatched_poll),
        Offer::Acquire {
            message_id: 7,
            ids: vec![derived_id, derived_id],
        }
    );
    Ok(())
}

struct RecordingOfferTransport {
    acquire_calls: usize,
}

impl Transport for RecordingOfferTransport {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        match (request.method, request.path.as_str()) {
            (Method::Get, "/repos/owner/repo/actions/runs/100") => Ok(Exchange {
                status: 200,
                body: serde_json::to_vec(&json!({
                    "id": 100,
                    "event": "push",
                    "head_branch": "main",
                    "head_sha": SOURCE_SHA,
                    "path": "owner/repo/.github/workflows/ci.yml@refs/heads/main",
                    "workflow_id": 1,
                    "run_attempt": 1,
                    "repository": {
                        "id": 42,
                        "name": "repo",
                        "full_name": "owner/repo",
                        "owner": { "login": "owner" }
                    },
                    "head_repository": {
                        "id": 42,
                        "name": "repo",
                        "full_name": "owner/repo",
                        "owner": { "login": "owner" }
                    },
                    "referenced_workflows": []
                }))
                .map_err(|_| TransportFail::Http(500))?,
            }),
            (Method::Post, "_apis/runtime/runnerscalesets/7/acquirejobs") => {
                self.acquire_calls += 1;
                let value: Vec<i64> =
                    serde_json::from_slice(&request.body).map_err(|_| TransportFail::Http(400))?;
                Ok(Exchange {
                    status: 200,
                    body: serde_json::to_vec(&json!({
                        "count": value.len(),
                        "value": value,
                    }))
                    .map_err(|_| TransportFail::Http(500))?,
                })
            }
            _ => Err(TransportFail::Http(404)),
        }
    }
}

fn job_poll_with_source(
    message_id: i64,
    message_types: &[&str],
    job_id: &str,
) -> TestResult<velnor_runner_github::ParsedBatch> {
    let entries: Vec<serde_json::Value> = message_types
        .iter()
        .map(|message_type| {
            json!({
                "messageType": message_type,
                "runnerRequestId": 0,
                "jobId": job_id,
                "repositoryName": "repo",
                "ownerName": "owner",
                "jobWorkflowRef": "owner/repo/.github/workflows/ci.yml@refs/heads/main",
                "workflowRunId": 100,
                "eventName": "push"
            })
        })
        .collect();
    let body = serde_json::to_string(&entries)?;
    let envelope = serde_json::to_string(&json!({
        "messageId": message_id,
        "messageType": "RunnerScaleSetJobMessages",
        "body": body
    }))?;
    let Poll::Batch(batch) = parse_poll(200, &envelope)? else {
        return Err(std::io::Error::other("expected poll batch").into());
    };
    Ok(batch)
}

#[test]
fn public_deferred_assigned_offer_skips_acquirejobs_on_resumption() -> TestResult<()> {
    let temp = tempfile::tempdir()?;
    let journal_path = temp.path().join("host.db");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let connection = sample_connection(vec![WorkflowEvent::Push], Vec::new())?;
    let mut transport = RecordingOfferTransport { acquire_calls: 0 };

    let assigned_batch = job_poll_with_source(
        10,
        &["JobAvailable", "JobAssigned"],
        "67c5c946-116b-52aa-9566-1d8de67a2a67",
    )?;
    let assigned_id = assigned_batch.jobs[0]
        .request_id
        .expect("assigned request_id");
    let skipped: TestResult<bool> = runtime.block_on(async {
        let journal = Journal::open(&journal_path).await?;
        Ok(resume_deferred_offer_acquire_with_transport(
            &journal,
            &connection,
            7,
            "ghs_test_token",
            &assigned_batch,
            assigned_id,
            &mut transport,
        )
        .await?)
    });
    assert!(skipped?);
    assert_eq!(transport.acquire_calls, 0);

    let available_batch = job_poll_with_source(
        11,
        &["JobAvailable"],
        "11111111-2222-3333-4444-555555555555",
    )?;
    let available_id = available_batch.jobs[0]
        .request_id
        .expect("available request_id");
    let skipped_available: TestResult<bool> = runtime.block_on(async {
        let journal = Journal::open(&journal_path).await?;
        Ok(resume_deferred_offer_acquire_with_transport(
            &journal,
            &connection,
            7,
            "ghs_test_token",
            &available_batch,
            available_id,
            &mut transport,
        )
        .await?)
    });
    assert!(!skipped_available?);
    assert_eq!(transport.acquire_calls, 1);
    Ok(())
}
