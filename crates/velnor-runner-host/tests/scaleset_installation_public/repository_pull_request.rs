// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use velnor_runner_host::{
    ApprovedJobAuthorityPolicy, ConnectionId, HostConfig, HostError, JobAuthorityRole,
    KeychainReference, PullRequestRule, RepositoryConnection, ScaleSetSelector, SourcePolicy,
    SourcePolicyMode, WorkflowEvent, build_job_authority_policy_json,
    verify_persisted_execution_source_role,
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
    base_branch: &str,
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
        base_ref: Some(base_branch),
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
        "base_ref": base_branch,
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
    let inventory = inventory_json_for_contexts(&[TestInventoryContext {
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
        let source = pull_request_source_json(head_branch, 42, "owner/repo", "main")?;
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
    let fork = pull_request_source_json("feature", 43, "forker/repo", "main")?;
    assert_eq!(
        verify_persisted_execution_source_role(&fork, &policy, &connection)?,
        None
    );
    let missing_head = pull_request_source_json("feature", 0, "owner/repo", "main")?;
    assert!(matches!(
        verify_persisted_execution_source_role(&missing_head, &policy, &connection),
        Err(HostError::Identity)
    ));
    Ok(())
}

#[test]
fn public_repository_mode_matches_the_workflow_base_branches() -> TestResult<()> {
    let connection = repository_connection()?;
    let policy = repository_policy(&connection)?;
    let develop = pull_request_source_json("feature", 42, "owner/repo", "develop")?;
    assert_eq!(
        verify_persisted_execution_source_role(&develop, &policy, &connection)?,
        None
    );
    Ok(())
}

const HOST_TOML_HEAD: &str = "schema = 2\n\
[[connections]]\n\
connection_id = \"lab\"\n\
repository_id = 42\n\
repository = \"owner/repo\"\n\
credential_ref = \"keychain:com.tailrocks.velnor.host/lab\"\n\
[connections.scale_set]\n\
name = \"velnor-mac\"\n\
labels = [\"velnor\", \"velnor-mac\"]\n\
[connections.source_policy]\n";
const BRANCH_POLICY_TOML: &str = "mode = \"branch_restricted\"\n\
branches = [\"main\"]\n\
tag_prefixes = []\n\
events = [\"pull_request\"]\n\
pull_request_rules = [{ base_repository_id = 42, base_branch = \"main\", head_repository_id = 42, head_branch = \"feature\" }]\n";
const REPOSITORY_POLICY_TOML: &str = "mode = \"repository\"\n\
branches = []\n\
tag_prefixes = []\n\
events = [\"pull_request\"]\n\
pull_request_rules = []\n";
const HOST_TOML_TAIL: &str = "[host]\n\
launch_mode = \"rust-runner\"\n\
max_jobs = 1\n\
[host.resources]\n\
runner_cpu_millicores = 2000\n\
runner_memory_bytes = 4294967296\n\
dind_cpu_millicores = 1000\n\
dind_memory_bytes = 2147483648\n\
operational_memory_reserve_bytes = 1073741824\n\
[docker]\n\
context = \"orbstack\"\n\
platform = \"linux/arm64\"\n\
endpoint = \"unix:///var/run/docker.sock\"\n";

fn host_toml(policy: &str) -> String {
    format!("{HOST_TOML_HEAD}{policy}{HOST_TOML_TAIL}")
}

fn reseal(policy_json: &str) -> TestResult<String> {
    let hash_start = policy_json
        .find("\"policy_sha256\":\"")
        .ok_or("policy_sha256 member")?;
    let member_end = hash_start + "\"policy_sha256\":\"".len() + 64 + 2;
    let content = format!(
        "{}{}",
        &policy_json[..hash_start],
        &policy_json[member_end..]
    );
    let digest = hex_sha256(content.as_bytes());
    Ok(format!(
        "{}\"policy_sha256\":\"{digest}\",{}",
        &policy_json[..hash_start],
        &policy_json[member_end..]
    ))
}

fn same_repository_admission(policy_json: &str) -> TestResult<String> {
    let start = policy_json
        .find("\"pull_request\":{\"rules\":[")
        .ok_or("pull_request rules member")?;
    let end = start + policy_json[start..].find("]}").ok_or("rules end")? + 2;
    Ok(format!(
        "{}\"pull_request\":{{\"same_repository\":{{\"base_branches\":[\"main\"]}}}}{}",
        &policy_json[..start],
        &policy_json[end..]
    ))
}

#[test]
fn public_branch_restricted_connection_refuses_a_rule_free_pull_request_context() -> TestResult<()>
{
    let config = HostConfig::parse_draft(&host_toml(BRANCH_POLICY_TOML))?;
    let id = ConnectionId::parse("lab")?;
    let connection = config.connection(&id).ok_or("connection lab")?;
    let inventory = inventory_json_for_contexts(&[TestInventoryContext {
        event: "pull_request",
        branch_filters: Vec::new(),
        tag_filters: Vec::new(),
        pull_request_base_branches: vec!["main"],
    }])?;
    let genuine =
        build_job_authority_policy_json(connection, &inventory, SOURCE_SHA, &"d".repeat(64), 1)?;
    assert!(
        config
            .with_job_authority_policy_json(&id, &reseal(&genuine)?)
            .is_ok()
    );
    let forged = reseal(&same_repository_admission(&genuine)?)?;
    assert_eq!(
        config.with_job_authority_policy_json(&id, &forged).err(),
        Some(HostError::Config)
    );
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

fn repository_config_and_policy() -> TestResult<(HostConfig, ConnectionId, String)> {
    let config = HostConfig::parse_draft(&host_toml(REPOSITORY_POLICY_TOML))?;
    let id = ConnectionId::parse("lab")?;
    let connection = config.connection(&id).ok_or("connection lab")?;
    let inventory = inventory_json_for_contexts(&[TestInventoryContext {
        event: "pull_request",
        branch_filters: Vec::new(),
        tag_filters: Vec::new(),
        pull_request_base_branches: vec!["main"],
    }])?;
    let genuine =
        build_job_authority_policy_json(connection, &inventory, SOURCE_SHA, &"d".repeat(64), 1)?;
    Ok((config, id, genuine))
}

#[test]
fn public_repository_mode_refuses_a_trusted_publication_pull_request_context() -> TestResult<()> {
    let (config, id, genuine) = repository_config_and_policy()?;
    assert!(
        config
            .with_job_authority_policy_json(&id, &reseal(&genuine)?)
            .is_ok()
    );
    let trusted = genuine
        .replace(
            "\"role\":\"verification\"",
            "\"role\":\"trusted_publication\"",
        )
        .replace(
            "\"condition_sha256\":null",
            &format!("\"condition_sha256\":\"{}\"", "a".repeat(64)),
        )
        .replace(
            "\"concurrency\":null",
            "\"concurrency\":{\"scope\":\"job\",\"group\":\"g\",\"cancel_in_progress\":\"false\"}",
        );
    assert_ne!(trusted, genuine);
    assert_eq!(
        config
            .with_job_authority_policy_json(&id, &reseal(&trusted)?)
            .err(),
        Some(HostError::Config)
    );
    Ok(())
}

#[test]
fn public_outdated_job_authority_policy_asks_for_regeneration() -> TestResult<()> {
    let (config, id, genuine) = repository_config_and_policy()?;
    let expected = HostError::JobAuthorityPolicySchema {
        found: 1,
        expected: 2,
    };
    let outdated = genuine.replace("\"schema_version\":2", "\"schema_version\":1");
    assert_ne!(outdated, genuine);
    assert_eq!(
        config
            .with_job_authority_policy_json(&id, &reseal(&outdated)?)
            .err(),
        Some(expected)
    );
    assert!(
        expected
            .to_string()
            .contains("schema 1, expected 2; regenerate the job-authority policy")
    );
    let newer = genuine.replace("\"schema_version\":2", "\"schema_version\":3");
    assert_eq!(
        config
            .with_job_authority_policy_json(&id, &reseal(&newer)?)
            .err(),
        Some(HostError::JobAuthorityPolicySchema {
            found: 3,
            expected: 2
        })
    );
    let persisted = format!(
        "{}[[job_authority_policies]]\nschema_version = 1\npull_request_rules = []\n",
        host_toml(REPOSITORY_POLICY_TOML)
    );
    assert_eq!(HostConfig::parse_draft(&persisted).err(), Some(expected));
    Ok(())
}
