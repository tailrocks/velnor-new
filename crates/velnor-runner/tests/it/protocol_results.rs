// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result};
use serde_json::json;
use velnor_runner::protocol::{StepConclusion, StepStatus, TwirpResultsClient, TwirpStep};

use super::exec_support::Scratch;
use super::support::{Reply, Scripted, TOKEN, TestServer, assert_bearer, scripted};

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

pub(super) const RECEIVER_PATH: &str = "/twirp/results.services.receiver.Receiver";
pub(super) const RUN: &str = "run-backend";
pub(super) const JOB: &str = "job-backend";
pub(super) const STEP: &str = "step-1";

pub(super) fn receiver(method: &str) -> String {
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

    let scratch = Scratch::new()?;
    let summary = scratch.write("summary.md", b"# Summary\n")?;

    client.upload_step_summary(RUN, JOB, STEP, &summary).await?;

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
async fn blob_uploads_send_the_azure_header_only_for_azure_storage() -> Result<()> {
    for (storage, header) in [
        (Some("BLOB_STORAGE_TYPE_AZURE"), Some("BlockBlob")),
        (Some("BLOB_STORAGE_TYPE_UNSPECIFIED"), None),
        (Some("BLOB_STORAGE_TYPE_OTHER"), None),
        (None, None),
    ] {
        let server = TestServer::start().await?;
        let client = TwirpResultsClient::new(&server.uri(), TOKEN)?;
        let blob_url = format!("{}/blob/log", server.uri());
        server.respond("POST", &receiver("GetStepLogsSignedBlobURL"), move |_| {
            Reply::json(
                200,
                &json!({"logs_url": blob_url, "blob_storage_type": storage}),
            )
        })?;
        server.respond("PUT", "/blob/log", |_| Reply::status(201))?;
        server.respond("POST", &receiver("CreateStepLogsMetadata"), |_| {
            Reply::json(200, &json!({"ok": true}))
        })?;

        client
            .upload_step_log(RUN, JOB, STEP, &["one".to_owned()])
            .await?;

        let put = server.only_request("PUT", "/blob/log")?;
        assert_eq!(put.header("x-ms-blob-type"), header, "{storage:?}");
    }
    Ok(())
}

#[tokio::test]
async fn twirp_update_steps_sends_the_pending_status_as_five() -> Result<()> {
    let server = TestServer::start().await?;
    let client = TwirpResultsClient::new(&server.uri(), TOKEN)?;
    server.respond("POST", STEPS_UPDATE_PATH, |_| {
        Reply::json(200, &json!({"ok": true}))
    })?;
    let waiting = TwirpStep {
        status: StepStatus::Pending,
        conclusion: StepConclusion::Unknown,
        ..step("step-1")
    };

    client
        .update_steps(&[waiting], "run-backend", "job-backend", 1)
        .await?;

    let body = server.only_request("POST", STEPS_UPDATE_PATH)?.json()?;
    assert_eq!(body["steps"][0]["status"], 5);
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

    let scratch = Scratch::new()?;
    let summary = scratch.write("summary.md", b"longer than four bytes")?;

    let error = client
        .upload_step_summary(RUN, JOB, STEP, &summary)
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

    let scratch = Scratch::new()?;
    let log = scratch.write("step.log", b"one\ntwo\n")?;

    client.upload_step_log(RUN, JOB, STEP, &log).await?;

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
        Reply::json(200, &json!({"logs_url": blob_url, "blob_storage_type": "BLOB_STORAGE_TYPE_AZURE"}))
    })?;
    server.respond("PUT", "/blob/log", |_| Reply::status(201))?;
    server.respond("POST", &receiver("CreateStepLogsMetadata"), |_| {
        Reply::json(200, &json!({}))
    })?;

    let scratch = Scratch::new()?;
    let log = scratch.write("step.log", b"one\n")?;

    let error = client
        .upload_step_log(RUN, JOB, STEP, &log)
        .await
        .err()
        .context("metadata without ok must fail")?;

    assert!(format!("{error:#}").contains("ok=false"), "{error:#}");
    Ok(())
}

#[tokio::test]
async fn twirp_update_steps_retries_a_transient_failure_and_then_succeeds() -> Result<()> {
    let server = TestServer::start().await?;
    let client = TwirpResultsClient::new(&server.uri(), TOKEN)?;
    server.respond(
        "POST",
        STEPS_UPDATE_PATH,
        scripted(vec![
            Scripted::Text(500, "temporary"),
            Scripted::Json(200, json!({"ok": true})),
        ]),
    )?;

    client
        .update_steps(&[step("step-1")], "run-backend", "job-backend", 1)
        .await?;

    let requests = server.requests_to("POST", STEPS_UPDATE_PATH)?;
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].json()?, requests[1].json()?);
    Ok(())
}

#[tokio::test]
async fn twirp_update_steps_gives_up_after_three_attempts() -> Result<()> {
    let server = TestServer::start().await?;
    let client = TwirpResultsClient::new(&server.uri(), TOKEN)?;
    server.respond(
        "POST",
        STEPS_UPDATE_PATH,
        scripted(vec![Scripted::Text(500, "temporary")]),
    )?;

    let error = client
        .update_steps(&[step("step-1")], "run-backend", "job-backend", 1)
        .await
        .err()
        .context("a persistent 500 must fail")?;

    assert!(
        format!("{error:#}").contains("after 3 attempts"),
        "{error:#}"
    );
    assert_eq!(server.requests_to("POST", STEPS_UPDATE_PATH)?.len(), 3);
    Ok(())
}
