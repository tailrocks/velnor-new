// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write;

use anyhow::{Context, Result};
use serde_json::json;
use velnor_runner::protocol::{StepConclusion, StepStatus, TwirpResultsClient, TwirpStep};

use super::exec_support::Scratch;
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
        Reply::json(200, &json!({"logs_url": blob_url}))
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
async fn job_log_over_one_block_is_appended_in_blocks_without_losing_a_line() -> Result<()> {
    const LINES: usize = 150_000;
    let server = TestServer::start().await?;
    let client = TwirpResultsClient::new(&server.uri(), TOKEN)?;
    let blob_url = format!("{}/blob/log?sig=1", server.uri());
    server.respond("POST", &receiver("GetJobLogsSignedBlobURL"), move |_| {
        Reply::json(200, &json!({"logs_url": blob_url}))
    })?;
    server.respond("PUT", "/blob/log", |_| Reply::status(201))?;
    server.respond("POST", &receiver("CreateJobLogsMetadata"), |_| {
        Reply::json(200, &json!({"ok": true}))
    })?;
    let mut text = String::new();
    for number in 0..LINES {
        writeln!(text, "{number:020} {}", "x".repeat(40))?;
    }
    let scratch = Scratch::new()?;
    let log = scratch.write("job.log", text.as_bytes())?;

    client.upload_job_log(RUN, JOB, &log).await?;

    let puts = server.requests_to("PUT", "/blob/log")?;
    assert_eq!(puts[0].header("x-ms-blob-type"), Some("AppendBlob"));
    assert_eq!(puts[0].text(), "");
    let blocks: Vec<_> = puts
        .iter()
        .filter(|put| put.query("comp") == Some("appendblock"))
        .collect();
    assert!(blocks.len() >= 3, "{} blocks", blocks.len());
    for block in &blocks[..blocks.len() - 1] {
        let size = block.text().len();
        assert!(
            (2 * 1024 * 1024..2 * 1024 * 1024 + 128).contains(&size),
            "{size}"
        );
        assert!(block.text().ends_with('\n'));
    }
    let joined: String = blocks.iter().map(|block| block.text()).collect();
    assert_eq!(joined, text);
    let seal = puts.last().context("the seal request")?;
    assert_eq!(seal.query("comp"), Some("seal"));
    let metadata = server
        .only_request("POST", &receiver("CreateJobLogsMetadata"))?
        .json()?;
    assert_eq!(metadata["line_count"], LINES);
    Ok(())
}

#[tokio::test]
async fn job_log_of_one_block_is_a_single_block_blob() -> Result<()> {
    let server = TestServer::start().await?;
    let client = TwirpResultsClient::new(&server.uri(), TOKEN)?;
    let blob_url = format!("{}/blob/log", server.uri());
    server.respond("POST", &receiver("GetJobLogsSignedBlobURL"), move |_| {
        Reply::json(200, &json!({"logs_url": blob_url}))
    })?;
    server.respond("PUT", "/blob/log", |_| Reply::status(201))?;
    server.respond("POST", &receiver("CreateJobLogsMetadata"), |_| {
        Reply::json(200, &json!({"ok": true}))
    })?;
    let scratch = Scratch::new()?;
    let log = scratch.write("job.log", b"one\ntwo\nthree\n")?;

    client.upload_job_log(RUN, JOB, &log).await?;

    let put = server.only_request("PUT", "/blob/log")?;
    assert_eq!(put.header("x-ms-blob-type"), Some("BlockBlob"));
    assert_eq!(put.query("comp"), None);
    assert_eq!(put.text(), "one\ntwo\nthree\n");
    let metadata = server
        .only_request("POST", &receiver("CreateJobLogsMetadata"))?
        .json()?;
    assert_eq!(metadata["line_count"], 3);
    Ok(())
}
