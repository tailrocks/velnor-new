//! Tests for authenticated source-to-request matching.

use super::*;
use crate::launch::execution_authorization::{
    ExecutionProofChallenge, ExecutionProofChallengeFields,
};
use serde_json::{Value, json};

const ROOT_PATH: &str = ".github/workflows/build.yml";

#[test]
fn live_source_wrapper_keeps_the_verified_channel_digest() {
    let channel_digest = repeated('8', 64);
    let source = test_authorized_execution_source(pull_request_source(), channel_digest.clone())
        .expect("source fixture validates");
    assert_eq!(source.channel_digest(), channel_digest);
    assert!(source.matches_challenge(&challenge(repeated('8', 64))));
    assert!(!source.matches_challenge(&challenge(repeated('7', 64))));
}

#[test]
fn pull_request_source_requires_exact_repository_commit_and_ref_facts() {
    let source = pull_request_source();
    assert!(source.validate().is_ok());
    assert_eq!(source.tested_commit_sha, source.request.tested_sha());
    assert_eq!(source.root_commit_sha, source.request.workflow_sha());

    let mut altered = pull_request_source();
    altered.tested_commit_sha = repeated('d', 40);
    assert!(altered.validate().is_err());

    let mut altered = pull_request_source();
    set_request(&mut altered, "tested_sha", json!(repeated('d', 40)));
    assert!(altered.validate().is_err());

    let mut altered = pull_request_source();
    altered
        .pull_request_source
        .as_mut()
        .expect("pull request facts exist")
        .base_repository_id = 900;
    assert!(altered.validate().is_err());

    let mut altered = pull_request_source();
    altered
        .pull_request_source
        .as_mut()
        .expect("pull request facts exist")
        .base_repository_name = "other/project".into();
    assert!(altered.validate().is_err());

    let mut altered = pull_request_source();
    altered
        .pull_request_source
        .as_mut()
        .expect("pull request facts exist")
        .number = 20;
    assert!(altered.validate().is_err());

    let mut altered = pull_request_source();
    set_request(&mut altered, "base_ref", json!("feature"));
    assert!(altered.validate().is_err());

    let mut altered = pull_request_source();
    set_request(&mut altered, "base_sha", json!(repeated('c', 40)));
    assert!(altered.validate().is_err());

    let mut altered = pull_request_source();
    set_request(&mut altered, "head_ref", json!("main"));
    assert!(altered.validate().is_err());

    let mut altered = pull_request_source();
    set_request(&mut altered, "head_sha", json!(repeated('b', 40)));
    assert!(altered.validate().is_err());

    let mut altered = pull_request_source();
    set_request(&mut altered, "pull_request_number", json!(20));
    assert!(altered.validate().is_err());

    let mut altered = pull_request_source();
    set_request(&mut altered, "event_ref", json!("refs/pull/20/merge"));
    assert!(altered.validate().is_err());

    let mut altered = pull_request_source();
    set_request(&mut altered, "repository_id", json!(502));
    assert!(altered.validate().is_err());

    let mut altered = pull_request_source();
    set_request(&mut altered, "repository_name", json!("other/project"));
    assert!(altered.validate().is_err());

    let mut altered = pull_request_source();
    set_request(
        &mut altered,
        "workflow_ref",
        json!("acme/project/.github/workflows/build.yml@refs/pull/20/merge"),
    );
    assert!(altered.validate().is_err());

    let mut altered = pull_request_source();
    altered
        .pull_request_source
        .as_mut()
        .expect("pull request facts exist")
        .merge_ref = "refs/pull/20/merge".into();
    assert!(altered.validate().is_err());

    let mut altered = pull_request_source();
    altered
        .pull_request_source
        .as_mut()
        .expect("pull request facts exist")
        .merge_commit_sha = repeated('d', 40);
    assert!(altered.validate().is_err());
}

#[test]
fn pull_request_requires_complete_authenticated_facts_and_workflow_sha() {
    let mut altered = pull_request_source();
    altered.pull_request_source = None;
    assert!(altered.validate().is_err());

    let mut altered = pull_request_source();
    set_request(&mut altered, "base_sha", Value::Null);
    assert!(altered.validate().is_err());

    let mut altered = pull_request_source();
    set_request(&mut altered, "head_sha", Value::Null);
    assert!(altered.validate().is_err());

    let mut altered = pull_request_source();
    set_request(&mut altered, "workflow_sha", json!(repeated('a', 40)));
    assert!(altered.validate().is_err());

    let mut altered = pull_request_source();
    altered.root_commit_sha = repeated('a', 40);
    assert!(altered.validate().is_err());
}

#[test]
fn non_pull_request_source_binds_tested_commit_and_rejects_pr_metadata() {
    let mut source = pull_request_source();
    source.request = request(json!({
        "schema_version": 1,
        "challenge": repeated('9', 64),
        "reservation_id": 11,
        "intent_id": 12,
        "repository_id": 501,
        "repository_name": "acme/project",
        "event_name": "push",
        "event_ref": "refs/heads/main",
        "tested_sha": repeated('a', 40),
        "run_id": 13,
        "run_attempt": 1,
        "job_key": "verify",
        "workflow_ref": "acme/project/.github/workflows/build.yml@refs/heads/main",
        "workflow_sha": repeated('f', 40),
        "base_ref": null,
        "base_sha": null,
        "head_ref": null,
        "head_sha": null,
        "pull_request_number": null
    }));
    source.request_digest = source.request.sha256().expect("request digest");
    source.pull_request_source = None;
    assert!(source.validate().is_ok());

    source.pull_request_source = Some(pr_facts());
    assert!(source.validate().is_err());

    source.pull_request_source = None;
    source.tested_commit_sha = repeated('b', 40);
    assert!(source.validate().is_err());
}

fn pull_request_source() -> PersistedExecutionSource {
    let request = request(json!({
        "schema_version": 1,
        "challenge": repeated('9', 64),
        "reservation_id": 11,
        "intent_id": 12,
        "repository_id": 501,
        "repository_name": "acme/project",
        "event_name": "pull_request",
        "event_ref": "refs/pull/19/merge",
        "tested_sha": repeated('a', 40),
        "run_id": 13,
        "run_attempt": 1,
        "job_key": "verify",
        "workflow_ref": "acme/project/.github/workflows/build.yml@refs/pull/19/merge",
        "workflow_sha": repeated('f', 40),
        "base_ref": "main",
        "base_sha": repeated('b', 40),
        "head_ref": "feature",
        "head_sha": repeated('c', 40),
        "pull_request_number": 19
    }));
    let mut source = PersistedExecutionSource {
        request,
        request_digest: String::new(),
        tested_commit_sha: repeated('a', 40),
        root_workflow_path: ROOT_PATH.into(),
        root_commit_sha: repeated('f', 40),
        root_tree_sha: repeated('e', 40),
        root_blob_sha: repeated('d', 40),
        root_content_sha256: repeated('c', 64),
        leaf_workflow_path: ROOT_PATH.into(),
        leaf_commit_sha: repeated('f', 40),
        leaf_tree_sha: repeated('e', 40),
        leaf_blob_sha: repeated('d', 40),
        leaf_content_sha256: repeated('c', 64),
        workflow_closure: vec![VerifiedWorkflowIdentity {
            repository_id: 501,
            path: ROOT_PATH.into(),
            commit_sha: repeated('f', 40),
            tree_sha: repeated('e', 40),
            blob_sha: repeated('d', 40),
            content_sha256: repeated('c', 64),
        }],
        pull_request_source: Some(pr_facts()),
        rest_job_id: Some("rest-job-17".into()),
    };
    source.request_digest = source.request.sha256().expect("request digest");
    source
}

fn pr_facts() -> PersistedPullRequestSource {
    PersistedPullRequestSource {
        number: 19,
        base_repository_id: 501,
        base_repository_name: "acme/project".into(),
        base_ref: "main".into(),
        base_sha: repeated('b', 40),
        head_repository_id: 900,
        head_repository_name: "fork/project".into(),
        head_ref: "feature".into(),
        head_sha: repeated('c', 40),
        merge_ref: "refs/pull/19/merge".into(),
        merge_commit_sha: repeated('a', 40),
    }
}

fn request(value: Value) -> ExecutionSourceRequest {
    serde_json::from_value(value).expect("valid request fixture")
}

fn set_request(source: &mut PersistedExecutionSource, name: &str, value: Value) {
    let mut request = serde_json::to_value(&source.request).expect("serialize request fixture");
    request[name] = value;
    source.request = serde_json::from_value(request).expect("deserialize request fixture");
    source.request_digest = source.request.sha256().expect("request digest");
}

fn repeated(byte: char, length: usize) -> String {
    std::iter::repeat_n(byte, length).collect()
}

fn challenge(channel_digest: String) -> ExecutionProofChallenge {
    ExecutionProofChallenge::from_journal(ExecutionProofChallengeFields {
        challenge: repeated('9', 64),
        reservation_id: 11,
        intent_id: 12,
        witness_request_id: 17,
        runner_id: 23,
        runner_name: "runner".into(),
        runner_scale_set_id: 29,
        runner_container_id: "runner-container-id".into(),
        dind_container_id: "dind-container-id".into(),
        engine_id: "engine".into(),
        docker_root_digest: repeated('e', 64),
        instance_binding_digest: repeated('f', 64),
        policy_generation: 7,
        policy_digest: repeated('a', 64),
        channel_digest,
    })
    .expect("valid challenge fixture")
}
