// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write;
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::json;
use velnor_runner::protocol::TwirpResultsClient;

use super::exec_support::Scratch;
use super::protocol_results::{JOB, RUN, STEP, receiver};
use super::support::{RecordedRequest, Reply, TOKEN, TestServer};

#[tokio::test]
async fn job_log_over_one_block_is_appended_in_blocks_without_losing_a_line() -> Result<()> {
    const LINES: usize = 150_000;
    let server = TestServer::start().await?;
    let client = TwirpResultsClient::new(&server.uri(), TOKEN)?;
    let blob_url = format!("{}/blob/log?sig=1", server.uri());
    server.respond("POST", &receiver("GetJobLogsSignedBlobURL"), move |_| {
        Reply::json(200, &json!({"logs_url": blob_url, "blob_storage_type": "BLOB_STORAGE_TYPE_AZURE"}))
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
    assert_eq!(puts[0].query("comp"), None);
    for put in &puts {
        assert_eq!(put.query("sig"), Some("1"));
        assert_eq!(put.header("x-ms-version"), None, "{}", put.path);
    }
    let (last, blocks) = puts[1..].split_last().context("the blocks")?;
    assert!(blocks.len() >= 2, "{} blocks before the last", blocks.len());
    for block in blocks {
        assert_eq!(block.query("comp"), Some("appendblock"));
        assert_eq!(block.query("seal"), None);
        assert_eq!(block.header("x-ms-blob-sealed"), Some("False"));
        let size = block.text().len();
        assert!(
            (2 * 1024 * 1024..2 * 1024 * 1024 + 128).contains(&size),
            "{size}"
        );
        assert!(block.text().ends_with('\n'));
    }
    assert_eq!(last.query("comp"), Some("appendblock"));
    assert_eq!(last.query("seal"), Some("true"));
    assert_eq!(last.header("x-ms-blob-sealed"), Some("True"));
    assert_eq!(
        last.header("content-length"),
        Some(last.text().len().to_string().as_str())
    );
    let joined: String = puts.iter().map(RecordedRequest::text).collect();
    assert_eq!(joined, text);
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
        Reply::json(200, &json!({"logs_url": blob_url, "blob_storage_type": "BLOB_STORAGE_TYPE_AZURE"}))
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

#[tokio::test]
async fn job_log_whose_seal_fails_fails_the_upload_and_reports_no_metadata() -> Result<()> {
    let server = TestServer::start().await?;
    let client = TwirpResultsClient::new(&server.uri(), TOKEN)?;
    let blob_url = format!("{}/blob/log?sig=1", server.uri());
    server.respond("POST", &receiver("GetJobLogsSignedBlobURL"), move |_| {
        Reply::json(200, &json!({"logs_url": blob_url, "blob_storage_type": "BLOB_STORAGE_TYPE_AZURE"}))
    })?;
    server.respond("PUT", "/blob/log", |put| {
        Reply::status(if put.query("seal").is_some() {
            500
        } else {
            201
        })
    })?;
    server.respond("POST", &receiver("CreateJobLogsMetadata"), |_| {
        Reply::json(200, &json!({"ok": true}))
    })?;
    let text = format!("{}\n", "x".repeat(40)).repeat(80_000);
    let scratch = Scratch::new()?;
    let log = scratch.write("job.log", text.as_bytes())?;

    let error = client
        .upload_job_log(RUN, JOB, &log)
        .await
        .err()
        .context("a failed seal must fail the upload")?;

    assert!(format!("{error:#}").contains("status=500"), "{error:#}");
    assert!(
        server
            .requests_to("PUT", "/blob/log")?
            .iter()
            .any(|put| put.query("seal") == Some("true"))
    );
    assert!(
        server
            .requests_to("POST", &receiver("CreateJobLogsMetadata"))?
            .is_empty()
    );
    Ok(())
}

#[tokio::test]
async fn step_summary_exactly_at_the_soft_limit_is_uploaded() -> Result<()> {
    let server = TestServer::start().await?;
    let client = TwirpResultsClient::new(&server.uri(), TOKEN)?;
    let blob_url = format!("{}/blob/summary", server.uri());
    server.respond(
        "POST",
        &receiver("GetStepSummarySignedBlobURL"),
        move |_| {
            Reply::json(
                200,
                &json!({"summary_url": blob_url, "soft_size_limit": 10}),
            )
        },
    )?;
    server.respond("PUT", "/blob/summary", |_| Reply::status(201))?;
    server.respond("POST", &receiver("CreateStepSummaryMetadata"), |_| {
        Reply::json(200, &json!({"ok": true}))
    })?;
    let scratch = Scratch::new()?;
    let summary = scratch.write("summary.md", b"0123456789")?;

    client.upload_step_summary(RUN, JOB, STEP, &summary).await?;

    assert_eq!(
        server.only_request("PUT", "/blob/summary")?.text(),
        "0123456789"
    );
    Ok(())
}

#[tokio::test]
async fn log_block_slower_than_the_block_budget_fails_the_upload() -> Result<()> {
    let server = TestServer::start().await?;
    let client = TwirpResultsClient::new(&server.uri(), TOKEN)?
        .with_block_budget(Duration::from_millis(200));
    let blob_url = format!("{}/blob/log", server.uri());
    server.respond("POST", &receiver("GetJobLogsSignedBlobURL"), move |_| {
        Reply::json(200, &json!({"logs_url": blob_url, "blob_storage_type": "BLOB_STORAGE_TYPE_AZURE"}))
    })?;
    server.respond("PUT", "/blob/log", |_| {
        Reply::status(201).after(Duration::from_millis(600))
    })?;
    server.respond("POST", &receiver("CreateJobLogsMetadata"), |_| {
        Reply::json(200, &json!({"ok": true}))
    })?;
    let scratch = Scratch::new()?;
    let log = scratch.write("job.log", b"one\n")?;

    let error = client
        .upload_job_log(RUN, JOB, &log)
        .await
        .err()
        .context("a block over its budget must fail")?;

    assert!(format!("{error:#}").contains("PUT"), "{error:#}");
    assert!(
        server
            .requests_to("POST", &receiver("CreateJobLogsMetadata"))?
            .is_empty()
    );
    Ok(())
}
