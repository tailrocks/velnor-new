// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "offer_and_assigned.rs"]
mod offer_and_assigned;

use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use velnor_runner_host::{
    ApprovedJobAuthorityPolicy, BootstrapContainerRole, ConnectionId, HostError, JobAuthorityRole,
    KeychainReference, RepositoryConnection, ScaleSetSelector, SourcePolicy, SourcePolicyMode,
    WorkerImage, WorkflowEvent, build_job_authority_policy_json, runner_pair_environment,
    verify_persisted_execution_source_role, verify_worker_runtime_config,
};

use super::{SOURCE_SHA, TestResult, WRONG_SOURCE_SHA};

fn worker_runtime_config_value(image: WorkerImage, kind: Option<&str>, revision: &str) -> Value {
    let mut labels = json!({
        "org.opencontainers.image.created": "2026-01-01T00:00:00Z",
        "org.opencontainers.image.description": "Ubuntu 26.04",
        "org.opencontainers.image.title": "ubuntu",
        "org.opencontainers.image.version": "26.04",
        "org.opencontainers.image.revision": revision,
    });
    if let (Some(map), Some(kind_value)) = (labels.as_object_mut(), kind) {
        map.insert("io.velnor.image.kind".to_owned(), json!(kind_value));
    }
    match image {
        WorkerImage::Runner => json!({
            "User": "runner",
            "Env": ["PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"],
            "Entrypoint": ["/usr/local/bin/velnor-runner-entrypoint"],
            "WorkingDir": "/home/runner",
            "Labels": labels,
        }),
        WorkerImage::Dind => json!({
            "Env": ["PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"],
            "Entrypoint": ["/usr/local/bin/velnor-dind-entrypoint"],
            "Labels": labels,
        }),
    }
}

#[test]
fn public_runner_pair_env_and_worker_runtime_config_enforce_socket_hook_and_labels()
-> TestResult<()> {
    let runner_env = runner_pair_environment(BootstrapContainerRole::Runner)
        .ok_or_else(|| std::io::Error::other("missing runner env"))?;
    assert_eq!(
        runner_env,
        vec![
            "DOCKER_HOST=unix:///var/run/docker.sock".to_owned(),
            "ACTIONS_RUNNER_HOOK_JOB_STARTED=/usr/local/bin/velnor-job-start-hook.sh".to_owned(),
        ]
    );
    assert!(!runner_env.iter().any(|entry| entry.contains("tcp://")));
    assert_eq!(runner_pair_environment(BootstrapContainerRole::Dind), None);

    let runner_cfg = worker_runtime_config_value(WorkerImage::Runner, Some("runner"), SOURCE_SHA);
    verify_worker_runtime_config(&runner_cfg, WorkerImage::Runner, SOURCE_SHA)?;

    let dind_cfg = worker_runtime_config_value(WorkerImage::Dind, Some("dind"), SOURCE_SHA);
    verify_worker_runtime_config(&dind_cfg, WorkerImage::Dind, SOURCE_SHA)?;

    let missing_kind = worker_runtime_config_value(WorkerImage::Runner, None, SOURCE_SHA);
    let wrong_kind = worker_runtime_config_value(WorkerImage::Runner, Some("dind"), SOURCE_SHA);
    let wrong_rev =
        worker_runtime_config_value(WorkerImage::Runner, Some("runner"), WRONG_SOURCE_SHA);
    for invalid in [&missing_kind, &wrong_kind, &wrong_rev] {
        assert!(matches!(
            verify_worker_runtime_config(invalid, WorkerImage::Runner, SOURCE_SHA),
            Err(HostError::Docker)
        ));
    }
    Ok(())
}

fn hex_sha256(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

pub(super) fn sample_connection(
    events: Vec<WorkflowEvent>,
    tag_prefixes: Vec<String>,
) -> TestResult<RepositoryConnection> {
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
            SourcePolicyMode::BranchRestricted,
            vec!["main".to_owned()],
            tag_prefixes,
            events,
            Vec::new(),
        )?,
    )?)
}

#[derive(Clone, Serialize)]
struct TestInventoryContext {
    event: &'static str,
    branch_filters: Vec<&'static str>,
    tag_filters: Vec<&'static str>,
    pull_request_base_branches: Vec<&'static str>,
}

#[derive(Serialize)]
struct TestInventoryJob {
    root_path: &'static str,
    workflow_path: &'static str,
    job_key: &'static str,
    role: &'static str,
    contexts: Vec<TestInventoryContext>,
    permissions_declared: bool,
    permissions: std::collections::BTreeMap<&'static str, &'static str>,
    secrets: Vec<&'static str>,
    environments: Vec<&'static str>,
    condition_sha256: Option<&'static str>,
    concurrency: Option<&'static str>,
    needs: Vec<&'static str>,
}

#[derive(Serialize)]
struct TestInventoryWorkflow {
    path: &'static str,
    content_sha256: String,
    calls: Vec<&'static str>,
    jobs: Vec<TestInventoryJob>,
}

#[derive(Serialize)]
struct TestInventoryContent<'a> {
    schema_version: u32,
    generator_source_sha: &'a str,
    workflows: &'a [TestInventoryWorkflow],
}

#[derive(Serialize)]
struct TestInventoryEnvelope<'a> {
    schema_version: u32,
    generator_source_sha: &'a str,
    inventory_sha256: String,
    workflows: &'a [TestInventoryWorkflow],
}

fn inventory_json_for_contexts(contexts: Vec<TestInventoryContext>) -> TestResult<String> {
    let workflows = vec![TestInventoryWorkflow {
        path: ".github/workflows/ci.yml",
        content_sha256: "a".repeat(64),
        calls: Vec::new(),
        jobs: vec![TestInventoryJob {
            root_path: ".github/workflows/ci.yml",
            workflow_path: ".github/workflows/ci.yml",
            job_key: "build",
            role: "verification",
            contexts,
            permissions_declared: true,
            permissions: std::collections::BTreeMap::from([("contents", "read")]),
            secrets: Vec::new(),
            environments: Vec::new(),
            condition_sha256: None,
            concurrency: None,
            needs: Vec::new(),
        }],
    }];
    let content = TestInventoryContent {
        schema_version: 1,
        generator_source_sha: SOURCE_SHA,
        workflows: &workflows,
    };
    let inventory_sha256 = hex_sha256(&serde_json::to_vec(&content)?);
    Ok(serde_json::to_string(&TestInventoryEnvelope {
        schema_version: 1,
        generator_source_sha: SOURCE_SHA,
        inventory_sha256,
        workflows: &workflows,
    })?)
}

#[derive(Serialize)]
struct TestExecutionSourceRequest<'a> {
    schema_version: u32,
    challenge: String,
    reservation_id: i64,
    intent_id: i64,
    repository_id: u64,
    repository_name: &'a str,
    event_name: &'a str,
    event_ref: &'a str,
    tested_sha: &'a str,
    run_id: u64,
    run_attempt: u32,
    job_key: &'a str,
    workflow_ref: String,
    workflow_sha: &'a str,
    base_ref: Option<&'a str>,
    base_sha: Option<&'a str>,
    head_ref: Option<&'a str>,
    head_sha: Option<&'a str>,
    pull_request_number: Option<u64>,
}

fn persisted_execution_source_json(
    event_name: &str,
    event_ref: &str,
    workflow_ref_suffix: &str,
) -> TestResult<String> {
    let request = TestExecutionSourceRequest {
        schema_version: 1,
        challenge: "b".repeat(64),
        reservation_id: 1,
        intent_id: 2,
        repository_id: 42,
        repository_name: "owner/repo",
        event_name,
        event_ref,
        tested_sha: SOURCE_SHA,
        run_id: 100,
        run_attempt: 1,
        job_key: "build",
        workflow_ref: format!("owner/repo/.github/workflows/ci.yml@{workflow_ref_suffix}"),
        workflow_sha: SOURCE_SHA,
        base_ref: None,
        base_sha: None,
        head_ref: None,
        head_sha: None,
        pull_request_number: None,
    };
    let request_digest = hex_sha256(&serde_json::to_vec(&request)?);
    Ok(serde_json::to_string(&json!({
        "request": request,
        "request_digest": request_digest,
        "tested_commit_sha": SOURCE_SHA,
        "root_workflow_path": ".github/workflows/ci.yml",
        "root_commit_sha": SOURCE_SHA,
        "root_tree_sha": SOURCE_SHA,
        "root_blob_sha": SOURCE_SHA,
        "root_content_sha256": "a".repeat(64),
        "leaf_workflow_path": ".github/workflows/ci.yml",
        "leaf_commit_sha": SOURCE_SHA,
        "leaf_tree_sha": SOURCE_SHA,
        "leaf_blob_sha": SOURCE_SHA,
        "leaf_content_sha256": "a".repeat(64),
        "workflow_closure": [{
            "repository_id": 42,
            "path": ".github/workflows/ci.yml",
            "commit_sha": SOURCE_SHA,
            "tree_sha": SOURCE_SHA,
            "blob_sha": SOURCE_SHA,
            "content_sha256": "a".repeat(64)
        }],
        "pull_request_source": null,
        "rest_job_id": "job-1"
    }))?)
}

#[test]
fn public_persisted_execution_source_normalizes_merge_group_queue_ref_to_base_branch()
-> TestResult<()> {
    let connection = sample_connection(vec![WorkflowEvent::MergeGroup], Vec::new())?;
    let inventory_json = inventory_json_for_contexts(vec![TestInventoryContext {
        event: "merge_group",
        branch_filters: vec!["main"],
        tag_filters: Vec::new(),
        pull_request_base_branches: Vec::new(),
    }])?;
    let policy_json = build_job_authority_policy_json(
        &connection,
        &inventory_json,
        SOURCE_SHA,
        &"d".repeat(64),
        1,
    )?;
    let policy: ApprovedJobAuthorityPolicy = serde_json::from_str(&policy_json)?;

    let queue_ref = format!("refs/heads/gh-readonly-queue/main/pr-42-{SOURCE_SHA}");
    for workflow_ref_suffix in [queue_ref.as_str(), "refs/heads/main"] {
        let source_json =
            persisted_execution_source_json("merge_group", &queue_ref, workflow_ref_suffix)?;
        assert_eq!(
            verify_persisted_execution_source_role(&source_json, &policy, &connection)?,
            Some(JobAuthorityRole::Verification)
        );
    }

    let unapproved_queue_ref = format!("refs/heads/gh-readonly-queue/release/pr-42-{SOURCE_SHA}");
    let unapproved_json = persisted_execution_source_json(
        "merge_group",
        &unapproved_queue_ref,
        &unapproved_queue_ref,
    )?;
    assert_eq!(
        verify_persisted_execution_source_role(&unapproved_json, &policy, &connection)?,
        None
    );

    let malformed_queue_ref = "refs/heads/gh-readonly-queue/main/not-a-pr-ref";
    let malformed_json =
        persisted_execution_source_json("merge_group", malformed_queue_ref, malformed_queue_ref)?;
    assert!(matches!(
        verify_persisted_execution_source_role(&malformed_json, &policy, &connection),
        Err(HostError::Identity)
    ));
    Ok(())
}

#[test]
fn public_job_authority_policy_admits_release_and_workflow_dispatch_tags_under_scoped_prefix()
-> TestResult<()> {
    let connection = sample_connection(
        vec![WorkflowEvent::Release, WorkflowEvent::WorkflowDispatch],
        vec!["v".to_owned()],
    )?;
    let valid_inventory = inventory_json_for_contexts(vec![
        TestInventoryContext {
            event: "release",
            branch_filters: Vec::new(),
            tag_filters: vec!["v1.2.3"],
            pull_request_base_branches: Vec::new(),
        },
        TestInventoryContext {
            event: "workflow_dispatch",
            branch_filters: Vec::new(),
            tag_filters: vec!["v1.2.3"],
            pull_request_base_branches: Vec::new(),
        },
    ])?;
    let policy_json = build_job_authority_policy_json(
        &connection,
        &valid_inventory,
        SOURCE_SHA,
        &"d".repeat(64),
        1,
    )?;
    let policy: ApprovedJobAuthorityPolicy = serde_json::from_str(&policy_json)?;

    for event_name in ["release", "workflow_dispatch"] {
        let source_json =
            persisted_execution_source_json(event_name, "refs/tags/v1.2.3", "refs/tags/v1.2.3")?;
        assert_eq!(
            verify_persisted_execution_source_role(&source_json, &policy, &connection)?,
            Some(JobAuthorityRole::Verification)
        );
    }

    let unmatched_tag_inventory = inventory_json_for_contexts(vec![TestInventoryContext {
        event: "workflow_dispatch",
        branch_filters: Vec::new(),
        tag_filters: vec!["other-1.0.0"],
        pull_request_base_branches: Vec::new(),
    }])?;
    let release_with_branches_inventory =
        inventory_json_for_contexts(vec![TestInventoryContext {
            event: "release",
            branch_filters: vec!["main"],
            tag_filters: vec!["v1.2.3"],
            pull_request_base_branches: Vec::new(),
        }])?;
    for invalid_inventory in [&unmatched_tag_inventory, &release_with_branches_inventory] {
        assert!(matches!(
            build_job_authority_policy_json(
                &connection,
                invalid_inventory,
                SOURCE_SHA,
                &"d".repeat(64),
                1
            ),
            Err(HostError::Config)
        ));
    }
    assert!(matches!(
        SourcePolicy::try_new(
            SourcePolicyMode::Repository,
            Vec::new(),
            Vec::new(),
            vec![WorkflowEvent::PullRequest],
            Vec::new(),
        ),
        Err(HostError::Config)
    ));
    Ok(())
}
