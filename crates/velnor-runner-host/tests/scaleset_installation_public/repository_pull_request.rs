// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use velnor_runner_host::{
    ApprovedJobAuthorityPolicy, ConnectionId, HostError, JobAuthorityRole, KeychainReference,
    PullRequestRule, RepositoryConnection, ScaleSetSelector, SourcePolicy, SourcePolicyMode,
    WorkflowEvent, build_job_authority_policy_json, verify_persisted_execution_source_role,
};

use super::{
    TestExecutionSourceRequest, TestInventoryContext, hex_sha256, inventory_json_for_contexts,
    persisted_execution_source_json,
};
use crate::{SOURCE_SHA, TestResult};

fn repository_connection() -> TestResult<RepositoryConnection> {
    Ok(RepositoryConnection::try_new(
        ConnectionId::parse("conn-test-1")?,
        42,
        "owner/repo".to_owned(),
        KeychainReference::parse("keychain:velnor.test/conn-test-1")?,
        ScaleSetSelector::try_new(
            "ubuntu-26.04-scale-set",
            &["velnor".to_owned(), "ubuntu-26.04-scale-set".to_owned()],
        )?,
        SourcePolicy::try_new(
            SourcePolicyMode::Repository,
            Vec::new(),
            Vec::new(),
            vec![WorkflowEvent::PullRequest],
            Vec::new(),
        )?,
    )?)
}

fn pull_request_source_json(
    head_branch: &str,
    head_repository_id: i64,
    head_repository_name: &str,
) -> TestResult<String> {
    let base_sha = "b".repeat(40);
    let head_sha = "c".repeat(40);
    let request = TestExecutionSourceRequest {
        schema_version: 1,
        challenge: "b".repeat(64),
        reservation_id: 1,
        intent_id: 2,
        repository_id: 42,
        repository_name: "owner/repo",
        event_name: "pull_request",
        event_ref: "refs/pull/7/merge",
        tested_sha: SOURCE_SHA,
        run_id: 100,
        run_attempt: 1,
        job_key: "build",
        workflow_ref: "owner/repo/.github/workflows/ci.yml@refs/pull/7/merge".to_owned(),
        workflow_sha: SOURCE_SHA,
        base_ref: Some("main"),
        base_sha: Some(&base_sha),
        head_ref: Some(head_branch),
        head_sha: Some(&head_sha),
        pull_request_number: Some(7),
    };
    let mut source: Value = serde_json::from_str(&persisted_execution_source_json(
        "pull_request",
        "refs/pull/7/merge",
        "refs/pull/7/merge",
    )?)?;
    source["request_digest"] = json!(hex_sha256(&serde_json::to_vec(&request)?));
    source["request"] = serde_json::to_value(&request)?;
    source["pull_request_source"] = json!({
        "number": 7,
        "base_repository_id": 42,
        "base_repository_name": "owner/repo",
        "base_ref": "main",
        "base_sha": base_sha,
        "head_repository_id": head_repository_id,
        "head_repository_name": head_repository_name,
        "head_ref": head_branch,
        "head_sha": head_sha,
        "merge_ref": "refs/pull/7/merge",
        "merge_commit_sha": SOURCE_SHA,
    });
    Ok(serde_json::to_string(&source)?)
}

fn repository_policy(connection: &RepositoryConnection) -> TestResult<ApprovedJobAuthorityPolicy> {
    let inventory = inventory_json_for_contexts(vec![TestInventoryContext {
        event: "pull_request",
        branch_filters: Vec::new(),
        tag_filters: Vec::new(),
        pull_request_base_branches: vec!["main"],
    }])?;
    let policy_json =
        build_job_authority_policy_json(connection, &inventory, SOURCE_SHA, &"d".repeat(64), 1)?;
    Ok(serde_json::from_str(&policy_json)?)
}

#[test]
fn public_repository_mode_admits_same_repository_pull_request_from_any_head_branch()
-> TestResult<()> {
    let connection = repository_connection()?;
    let policy = repository_policy(&connection)?;
    for head_branch in ["feature", "topic/other"] {
        let source = pull_request_source_json(head_branch, 42, "owner/repo")?;
        assert_eq!(
            verify_persisted_execution_source_role(&source, &policy, &connection)?,
            Some(JobAuthorityRole::Verification),
            "{head_branch}"
        );
    }
    Ok(())
}

#[test]
fn public_repository_mode_refuses_fork_and_headless_pull_requests() -> TestResult<()> {
    let connection = repository_connection()?;
    let policy = repository_policy(&connection)?;
    let fork = pull_request_source_json("feature", 43, "forker/repo")?;
    assert_eq!(
        verify_persisted_execution_source_role(&fork, &policy, &connection)?,
        None
    );
    let missing_head = pull_request_source_json("feature", 0, "owner/repo")?;
    assert!(matches!(
        verify_persisted_execution_source_role(&missing_head, &policy, &connection),
        Err(HostError::Identity)
    ));
    Ok(())
}

#[test]
fn public_repository_mode_pull_request_policy_stays_rule_free() {
    let rule = PullRequestRule {
        number: None,
        base_repository_id: 42,
        base_branch: "main".to_owned(),
        head_repository_id: 42,
        head_branch: "feature".to_owned(),
    };
    assert!(matches!(
        SourcePolicy::try_new(
            SourcePolicyMode::Repository,
            Vec::new(),
            Vec::new(),
            vec![WorkflowEvent::PullRequest],
            vec![rule],
        ),
        Err(HostError::Config)
    ));
}
