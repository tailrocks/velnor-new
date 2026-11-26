// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::json;

use super::mock_github::{
    Behavior, JOB_LOG_BLOB, RESULTS_STEPS_UPDATE, STEP_LOG_BLOB, SUMMARY_BLOB, StepStatus,
};
use super::{RecordedRequest, Reply, TestServer};
use anyhow::Result;

pub(super) fn mount_results(server: &TestServer, base: &str, behavior: &Behavior) -> Result<()> {
    let (blob_status, seal_status, delay) = (
        behavior.blob_status,
        behavior.seal_status,
        behavior.blob_delay,
    );
    let blob_reply = move |request: &RecordedRequest| {
        let sealing = request.query("seal").is_some();
        Reply::status(seal_status.filter(|_| sealing).unwrap_or(blob_status)).after(delay)
    };
    let status_stalls = matches!(behavior.status, StepStatus::Stalls);
    server.respond("POST", RESULTS_STEPS_UPDATE, move |_| {
        if status_stalls {
            Reply::stall()
        } else {
            Reply::json(200, &json!({"ok": true}))
        }
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
        server.respond("PUT", blob, blob_reply)?;
    }
    let summary_url = format!("{base}{SUMMARY_BLOB}");
    server.respond(
        "POST",
        &format!("{receiver}/GetStepSummarySignedBlobURL"),
        move |_| Reply::json(200, &json!({"summary_url": summary_url})),
    )?;
    server.respond("PUT", SUMMARY_BLOB, blob_reply)?;
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
