// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::json;
use velnor_runner_github::{InnerKind, Poll, WireError, parse_inner_messages, parse_poll};

use super::envelope_with_statistics;

#[test]
fn zero_request_id_job_messages_derive_stable_request_id_and_normalize_empty_runner() {
    let body = serde_json::to_string(&json!([
        {
            "messageType": "JobAssigned",
            "runnerRequestId": 0,
            "jobId": "67c5c946-116b-52aa-9566-1d8de67a2a67",
            "scaleSetAssignTime": "2026-11-06T22:55:21.7495747Z"
        },
        {
            "messageType": "JobStarted",
            "runnerRequestId": 0,
            "jobId": "67c5c946-116b-52aa-9566-1d8de67a2a67",
            "scaleSetAssignTime": "2026-11-06T22:55:21.7495747Z",
            "runnerId": 29,
            "runnerName": "velnor-1"
        },
        {
            "messageType": "JobCompleted",
            "runnerRequestId": 0,
            "jobId": "67c5c946-116b-52aa-9566-1d8de67a2a67",
            "scaleSetAssignTime": "2026-11-06T22:55:21.7495747Z",
            "runnerId": 0,
            "runnerName": "",
            "result": "succeeded"
        },
        {
            "messageType": "JobAssigned",
            "runnerRequestId": 0,
            "jobId": "67c5c946-116b-52aa-9566-1d8de67a2a67",
            "scaleSetAssignTime": "2026-11-06T23:05:00.0000000Z"
        },
        {
            "messageType": "JobAssigned",
            "runnerRequestId": 0,
            "jobId": "11111111-2222-3333-4444-555555555555"
        }
    ]))
    .expect("serialize job messages");
    let Poll::Batch(batch) =
        parse_poll(200, &envelope_with_statistics(None, &body)).expect("parse zero-request batch")
    else {
        panic!("expected valid poll batch");
    };
    assert_eq!(batch.jobs.len(), 5);
    let derived_id = batch.jobs[0].request_id.expect("derived request_id");
    assert!(derived_id > 0);
    assert_eq!(batch.jobs[0].kind, InnerKind::Assigned);
    assert_eq!(batch.jobs[1].kind, InnerKind::Started);
    assert_eq!(batch.jobs[1].request_id, Some(derived_id));
    assert_eq!(batch.jobs[1].runner_id, Some(29));
    assert_eq!(batch.jobs[1].runner_name.as_deref(), Some("velnor-1"));
    assert_eq!(batch.jobs[2].kind, InnerKind::Completed);
    assert_eq!(batch.jobs[2].request_id, Some(derived_id));
    assert_eq!(batch.jobs[2].runner_id, None);
    assert_eq!(batch.jobs[2].runner_name, None);
    assert_eq!(batch.jobs[2].result.as_deref(), Some("succeeded"));
    let reassigned_id = batch.jobs[3].request_id.expect("reassigned request_id");
    assert!(reassigned_id > 0 && reassigned_id != derived_id);
    let other_id = batch.jobs[4].request_id.expect("other derived request_id");
    assert!(other_id > 0 && other_id != derived_id && other_id != reassigned_id);

    for invalid_body in [
        r#"[{"messageType":"JobAssigned","runnerRequestId":0}]"#,
        r#"[{"messageType":"JobAssigned","runnerRequestId":0,"jobId":""}]"#,
        r#"[{"messageType":"JobAssigned","runnerRequestId":0,"jobId":"67c5c946-116b-52aa-9566-1d8de67a2a67","scaleSetAssignTime":123}]"#,
    ] {
        assert_eq!(
            parse_inner_messages(invalid_body),
            Err(WireError::Malformed)
        );
        assert!(matches!(
            parse_poll(200, &envelope_with_statistics(None, invalid_body)),
            Ok(Poll::Quarantined(_))
        ));
    }
}
