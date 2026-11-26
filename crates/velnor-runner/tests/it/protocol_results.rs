// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;
use serde_json::json;
use velnor_runner::protocol::{StepConclusion, StepStatus, TwirpResultsClient, TwirpStep};

use super::support::{Reply, TOKEN, TestServer, assert_bearer};

const STEPS_UPDATE_PATH: &str =
    "/twirp/github.actions.results.api.v1.WorkflowStepUpdateService/WorkflowStepsUpdate";

#[tokio::test]
async fn twirp_update_steps_posts_workflow_steps_update() -> Result<()> {
    let server = TestServer::start().await?;
    let client = TwirpResultsClient::new(&server.uri(), TOKEN)?;
    server.respond("POST", STEPS_UPDATE_PATH, |_| Reply::json(200, &json!({})))?;

    let step = TwirpStep {
        external_id: "step-1".to_owned(),
        number: 1,
        name: "Run echo hello".to_owned(),
        status: StepStatus::Completed,
        started_at: Some("2026-11-27T00:00:00Z".to_owned()),
        completed_at: None,
        conclusion: StepConclusion::Success,
    };
    client
        .update_steps(&[step], "run-backend", "job-backend", 7)
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
    Ok(())
}
