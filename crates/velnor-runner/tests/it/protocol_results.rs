// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result};
use serde_json::json;
use velnor_runner::protocol::{StepConclusion, StepStatus, TwirpResultsClient, TwirpStep};

use super::support::{Reply, TOKEN, TestServer, assert_bearer};

const STEPS_UPDATE_PATH: &str =
    "/twirp/github.actions.results.api.v1.WorkflowStepUpdateService/WorkflowStepsUpdate";

#[tokio::test]
async fn twirp_update_steps_posts_workflow_steps_update() -> Result<()> {
    let server = TestServer::start().await?;
    let client = TwirpResultsClient::new(&server.uri(), TOKEN)?;
    server.respond("POST", STEPS_UPDATE_PATH, |_| {
        Reply::json(200, &json!({"ok": true}))
    })?;

    let background = TwirpStep {
        is_background: true,
        background_control_type: Some("wait".to_owned()),
        background_control_step_ids: Some(vec!["step-0".to_owned()]),
        parallel_group_id: Some("group-1".to_owned()),
        ..step("step-1")
    };
    client
        .update_steps(
            &[background, step("step-2")],
            "run-backend",
            "job-backend",
            7,
        )
        .await?;

    let request = server.only_request("POST", STEPS_UPDATE_PATH)?;
    assert_bearer(&request);
    let body = request.json()?;
    assert_eq!(body["workflow_run_backend_id"], "run-backend");
    assert_eq!(body["workflow_job_run_backend_id"], "job-backend");
    assert_eq!(body["change_order"], 7);
    assert_eq!(body["steps"][0]["external_id"], "step-1");
    assert_eq!(body["steps"][0]["status"], 6);
    assert_eq!(body["steps"][0]["conclusion"], 2);
    assert!(body["steps"][0].get("completed_at").is_none());
    assert_eq!(body["steps"][0]["is_background"], true);
    assert_eq!(body["steps"][0]["background_control_type"], "wait");
    assert_eq!(
        body["steps"][0]["background_control_step_ids"],
        json!(["step-0"])
    );
    assert_eq!(body["steps"][0]["parallel_group_id"], "group-1");
    for absent in [
        "is_background",
        "background_control_type",
        "background_control_step_ids",
        "parallel_group_id",
    ] {
        assert!(body["steps"][1].get(absent).is_none(), "{absent}");
    }
    Ok(())
}

#[tokio::test]
async fn twirp_update_steps_fails_unless_the_acknowledgement_is_ok() -> Result<()> {
    let server = TestServer::start().await?;
    let client = TwirpResultsClient::new(&server.uri(), TOKEN)?;

    for acknowledgement in [json!({"ok": false}), json!({})] {
        server.reset()?;
        server.respond("POST", STEPS_UPDATE_PATH, move |_| {
            Reply::json(200, &acknowledgement)
        })?;

        let error = client
            .update_steps(&[step("step-1")], "run-backend", "job-backend", 1)
            .await
            .err()
            .context("a rejected update must fail")?;

        assert!(format!("{error:#}").contains("ok=false"), "{error:#}");
    }
    Ok(())
}

fn step(external_id: &str) -> TwirpStep {
    TwirpStep {
        external_id: external_id.to_owned(),
        number: 1,
        name: "Run echo hello".to_owned(),
        status: StepStatus::Completed,
        started_at: Some("2026-11-27T00:00:00Z".to_owned()),
        completed_at: None,
        conclusion: StepConclusion::Success,
        is_background: false,
        background_control_type: None,
        background_control_step_ids: None,
        parallel_group_id: None,
    }
}

const RECEIVER_PATH: &str = "/twirp/results.services.receiver.Receiver";
const RUN: &str = "run-backend";
const JOB: &str = "job-backend";
const STEP: &str = "step-1";

fn receiver(method: &str) -> String {
    format!("{RECEIVER_PATH}/{method}")
}

#[tokio::test]
async fn step_summary_uploads_to_summary_url_and_reports_size() -> Result<()> {
    let server = TestServer::start().await?;
    let client = TwirpResultsClient::new(&server.uri(), TOKEN)?;
    let blob_url = format!("{}/blob/summary", server.uri());
    server.respond("POST", &receiver("GetStepSummarySignedBlobURL"), move |_| {
        Reply::json(
            200,
            &json!({"summary_url": blob_url, "soft_size_limit": "1024", "blob_storage_type": "BLOB_STORAGE_TYPE_AZURE"}),
        )
    })?;
    server.respond("PUT", "/blob/summary", |_| Reply::status(201))?;
    server.respond("POST", &receiver("CreateStepSummaryMetadata"), |_| {
        Reply::json(200, &json!({"ok": true}))
    })?;

    client
        .upload_step_summary(RUN, JOB, STEP, "# Summary\n")
        .await?;

    let signed = server.only_request("POST", &receiver("GetStepSummarySignedBlobURL"))?;
    assert_bearer(&signed);
    let signed_body = signed.json()?;
    assert_eq!(signed_body["workflow_run_backend_id"], RUN);
    assert_eq!(signed_body["workflow_job_run_backend_id"], JOB);
    assert_eq!(signed_body["step_backend_id"], STEP);
    let put = server.only_request("PUT", "/blob/summary")?;
    assert_eq!(put.header("x-ms-blob-type"), Some("BlockBlob"));
    let metadata = server
        .only_request("POST", &receiver("CreateStepSummaryMetadata"))?
        .json()?;
    assert_eq!(metadata["size"], "# Summary\n".len());
    assert_eq!(metadata["step_backend_id"], STEP);
    assert!(metadata.get("line_count").is_none());
    assert!(metadata["uploaded_at"].is_string());
    Ok(())
}

#[tokio::test]
async fn step_summary_over_soft_limit_is_rejected_without_upload_or_retry() -> Result<()> {
    let server = TestServer::start().await?;
    let client = TwirpResultsClient::new(&server.uri(), TOKEN)?;
    let blob_url = format!("{}/blob/summary", server.uri());
    server.respond(
        "POST",
        &receiver("GetStepSummarySignedBlobURL"),
        move |_| Reply::json(200, &json!({"summary_url": blob_url, "soft_size_limit": 4})),
    )?;

    let error = client
        .upload_step_summary(RUN, JOB, STEP, "longer than four bytes")
        .await
        .err()
        .context("oversized summary must fail")?;

    assert!(format!("{error:#}").contains("4 byte limit"), "{error:#}");
    assert_eq!(
        server
            .requests_to("POST", &receiver("GetStepSummarySignedBlobURL"))?
            .len(),
        1
    );
    assert!(server.requests_to("PUT", "/blob/summary")?.is_empty());
    assert!(
        server
            .requests_to("POST", &receiver("CreateStepSummaryMetadata"))?
            .is_empty()
    );
    Ok(())
}

#[tokio::test]
async fn step_log_uploads_to_logs_url_and_reports_line_count() -> Result<()> {
    let server = TestServer::start().await?;
    let client = TwirpResultsClient::new(&server.uri(), TOKEN)?;
    let blob_url = format!("{}/blob/log", server.uri());
    server.respond("POST", &receiver("GetStepLogsSignedBlobURL"), move |_| {
        Reply::json(200, &json!({"logs_url": blob_url, "soft_size_limit": 1024}))
    })?;
    server.respond("PUT", "/blob/log", |_| Reply::status(201))?;
    server.respond("POST", &receiver("CreateStepLogsMetadata"), |_| {
        Reply::json(200, &json!({"ok": true}))
    })?;

    client
        .upload_step_log(RUN, JOB, STEP, &["one".to_owned(), "two".to_owned()])
        .await?;

    let metadata = server
        .only_request("POST", &receiver("CreateStepLogsMetadata"))?
        .json()?;
    assert_eq!(metadata["line_count"], 2);
    assert!(metadata.get("size").is_none());
    Ok(())
}

#[tokio::test]
async fn step_log_metadata_without_ok_is_rejected() -> Result<()> {
    let server = TestServer::start().await?;
    let client = TwirpResultsClient::new(&server.uri(), TOKEN)?;
    let blob_url = format!("{}/blob/log", server.uri());
    server.respond("POST", &receiver("GetStepLogsSignedBlobURL"), move |_| {
        Reply::json(200, &json!({"logs_url": blob_url}))
    })?;
    server.respond("PUT", "/blob/log", |_| Reply::status(201))?;
    server.respond("POST", &receiver("CreateStepLogsMetadata"), |_| {
        Reply::json(200, &json!({}))
    })?;

    let error = client
        .upload_step_log(RUN, JOB, STEP, &["one".to_owned()])
        .await
        .err()
        .context("metadata without ok must fail")?;

    assert!(format!("{error:#}").contains("ok=false"), "{error:#}");
    Ok(())
}
