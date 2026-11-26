// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result};
use serde_json::json;
use velnor_runner::protocol::{
    DistributedTaskClient, TaskResult, TimelineRecord, TimelineRecordFeedLines, TimelineRecordType,
};

use super::support::{Reply, TOKEN, TestServer, assert_bearer};

const SCOPE: &str = "00000000-0000-0000-0000-0000000000a1";
const PLAN: &str = "00000000-0000-0000-0000-000000000002";
const TIMELINE: &str = "00000000-0000-0000-0000-000000000003";

#[tokio::test]
async fn timeline_update_records_patches_records() -> Result<()> {
    let server = TestServer::start().await?;
    let client = DistributedTaskClient::new(&format!("{}/vss", server.uri()), TOKEN)?;
    let records_path = format!(
        "/vss/{SCOPE}/_apis/distributedtask/hubs/Actions/plans/{PLAN}/timelines/{TIMELINE}/records"
    );
    let record = TimelineRecord::pending("step-1", TimelineRecordType::Task, "Run echo hello")
        .with_parent("job-1", 2)
        .in_progress("2026-11-27T00:00:00Z")
        .completed("2026-11-27T00:00:05Z", TaskResult::Succeeded);
    server.respond("PATCH", &records_path, |_| {
        Reply::json(
            200,
            &json!({
                "count": 1,
                "value": [{
                    "id": "step-1",
                    "type": "Task",
                    "name": "Run echo hello",
                    "state": "completed",
                    "result": "succeeded"
                }]
            }),
        )
    })?;

    let updated = client
        .update_timeline_records(SCOPE, "Actions", PLAN, TIMELINE, &[record])
        .await?;
    assert_eq!(updated.len(), 1);
    assert_eq!(updated[0].result, Some(TaskResult::Succeeded));

    let request = server.only_request("PATCH", &records_path)?;
    assert_bearer(&request);
    assert_eq!(request.query("api-version"), Some("5.1-preview.1"));
    let body = request.json()?;
    assert_eq!(body["count"], 1);
    let sent = body["value"].get(0).context("sent record")?;
    assert_eq!(sent["id"], "step-1");
    assert_eq!(sent["parentId"], "job-1");
    assert_eq!(sent["type"], "Task");
    assert_eq!(sent["state"], "completed");
    assert_eq!(sent["result"], "succeeded");
    Ok(())
}

#[tokio::test]
async fn timeline_record_feed_posts_count_beside_lines() -> Result<()> {
    let server = TestServer::start().await?;
    let client = DistributedTaskClient::new(&format!("{}/vss", server.uri()), TOKEN)?;
    let feed_path = format!(
        "/vss/{SCOPE}/_apis/distributedtask/hubs/Actions/plans/{PLAN}/timelines/{TIMELINE}/records/step-1/feed"
    );
    server.respond("POST", &feed_path, |_| Reply::status(200))?;

    let feed = TimelineRecordFeedLines::new(
        "step-1",
        vec!["first".to_owned(), "second".to_owned()],
        Some(5),
    );
    client
        .append_timeline_record_feed(SCOPE, "Actions", PLAN, TIMELINE, "step-1", &feed)
        .await?;

    let body = server.only_request("POST", &feed_path)?.json()?;
    assert_eq!(body["count"], 2);
    assert_eq!(body["stepId"], "step-1");
    assert_eq!(body["startLine"], 5);
    assert_eq!(body["value"][1], "second");
    Ok(())
}
