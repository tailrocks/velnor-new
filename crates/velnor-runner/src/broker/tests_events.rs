// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;

use super::proto::{Denied, Response};
use super::tests::{BRANCH, Fixture, facts_named, version};

const QUEUE: &str = r#"{"kind": "merge_group", "queue_branch": "gh-readonly-queue/main/pr-1-abc"}"#;

async fn is_denied_before_ready(name: &str, event: &str) -> Result<()> {
    let fixture = Fixture::from_facts(facts_named(name, event)?, |broker| broker)?;
    let refused = fixture.broker.begin_cache("k-1", &version()).await;
    assert!(
        matches!(refused, Err(Response::Denied(Denied::ReadOnlyEvent))),
        "{name}: {:?}",
        refused.err()
    );
    assert!(!fixture.incoming().exists());
    assert!(!fixture.root().join("cache").exists());
    Ok(())
}

#[tokio::test]
async fn a_pull_request_target_run_on_the_default_branch_cannot_write_its_scope() -> Result<()> {
    is_denied_before_ready("pull_request_target", BRANCH).await
}

#[tokio::test]
async fn a_workflow_run_on_the_default_branch_cannot_write_its_scope() -> Result<()> {
    is_denied_before_ready("workflow_run", BRANCH).await
}

#[tokio::test]
async fn an_issue_comment_run_on_the_default_branch_cannot_write_its_scope() -> Result<()> {
    is_denied_before_ready("issue_comment", BRANCH).await
}

#[tokio::test]
async fn a_merge_group_run_cannot_write_a_scope() -> Result<()> {
    is_denied_before_ready("merge_group", QUEUE).await
}
