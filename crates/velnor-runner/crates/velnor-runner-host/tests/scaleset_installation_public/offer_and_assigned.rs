// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::json;
use velnor_runner_github::{Poll, parse_poll};
use velnor_runner_host::{Offer, offer};

#[test]
fn public_offer_admits_assigned_batch_and_waits_on_lifecycle_only_batch() {
    let assigned_body = serde_json::to_string(&json!([{
        "messageType": "JobAssigned",
        "runnerRequestId": 0,
        "jobId": "67c5c946-116b-52aa-9566-1d8de67a2a67"
    }]))
    .expect("serialize assigned body");
    let assigned_envelope = serde_json::to_string(&json!({
        "messageId": 3,
        "messageType": "RunnerScaleSetJobMessages",
        "body": assigned_body
    }))
    .expect("serialize assigned envelope");
    let assigned_poll = parse_poll(200, &assigned_envelope).expect("parse assigned poll");
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
    ]))
    .expect("serialize lifecycle body");
    let lifecycle_envelope = serde_json::to_string(&json!({
        "messageId": 4,
        "messageType": "RunnerScaleSetJobMessages",
        "body": lifecycle_body
    }))
    .expect("serialize lifecycle envelope");
    let lifecycle_poll = parse_poll(200, &lifecycle_envelope).expect("parse lifecycle poll");
    assert_eq!(offer(&lifecycle_poll), Offer::Wait);
}
