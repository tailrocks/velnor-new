//! Checks that the hook belongs to the exact request that started.

use super::*;

use crate::config::ApprovedJobAuthorityPolicy;
use crate::journal::execution_authorization::decision::policy_snapshot::PolicySnapshot;
use crate::journal::execution_authorization::decision::proof_row::ProofRow;
use crate::journal::execution_authorization::decision::started::StartedExecution;
use crate::launch::execution_authorization::{
    ExecutionProofChallenge, ExecutionProofChallengeFields, ExecutionSourceRequest,
    PersistedExecutionSource, test_authorized_execution_source,
};
use serde::Serialize;
use serde_json::json;

const CONNECTION_ID: &str = "execution";
const REPOSITORY_ID: i64 = 101;
const SCALE_SET_ID: i64 = 501;
const INTENT_ID: i64 = 12;
const RESERVATION_ID: i64 = 11;
const WITNESS_REQUEST_ID: i64 = 100;
const RUNNER_ID: i64 = 701;
const WORKFLOW_PATH: &str = ".github/workflows/build.yml";
const ENGINE_ID: &str = "11111111-1111-4111-8111-111111111111";

#[test]
fn actual_started_request_can_equal_or_differ_from_the_jit_witness() {
    for actual_request_id in [WITNESS_REQUEST_ID, 101] {
        let run_id = 1_000 + actual_request_id;
        let source = execution_source(run_id, 1, "f");
        let stored_workflow_sha = (actual_request_id != WITNESS_REQUEST_ID).then_some("f");
        let started = started_execution(actual_request_id, run_id, 1, stored_workflow_sha);
        let proof = proof_row();
        let snapshot = policy_snapshot();

        let (role, deny_code) = determine_authority(&source, &proof, &started, &snapshot)
            .expect("the exact request source must be checked");
        assert_eq!(role, Some(crate::config::JobAuthorityRole::Verification));
        assert_eq!(deny_code, None);
    }
}

#[test]
fn inactive_journal_policy_snapshot_denies_matching_source() {
    let source = execution_source(1_101, 1, "f");
    let started = started_execution(101, 1_101, 1, Some("f"));
    let proof = proof_row();
    let mut snapshot = policy_snapshot();
    snapshot.is_active = false;

    let (role, deny_code) = determine_authority(&source, &proof, &started, &snapshot)
        .expect("An inactive policy must reject the matching source.");
    assert_eq!(role, None);
    assert_eq!(deny_code, Some("policy_changed"));
}

#[test]
fn mismatched_workflow_source_is_denied_before_policy_allowance() {
    let source = execution_source(1_101, 1, "e");
    let started = started_execution(101, 1_101, 1, Some("f"));
    let proof = proof_row();
    let snapshot = policy_snapshot();

    let (role, deny_code) = determine_authority(&source, &proof, &started, &snapshot)
        .expect("a valid but unrelated source is a denial");
    assert_eq!(role, None);
    assert_eq!(deny_code, Some("source_mismatch"));
}

#[test]
fn mismatched_run_attempt_is_denied_for_the_actual_request() {
    let source = execution_source(1_101, 2, "f");
    let started = started_execution(101, 1_101, 1, Some("f"));
    let proof = proof_row();
    let snapshot = policy_snapshot();

    let (role, deny_code) = determine_authority(&source, &proof, &started, &snapshot)
        .expect("a run-attempt mismatch is a denial");
    assert_eq!(role, None);
    assert_eq!(deny_code, Some("source_mismatch"));
}

fn execution_source(
    run_id: i64,
    run_attempt: u32,
    workflow_sha_byte: &str,
) -> AuthorizedExecutionSource {
    let workflow_sha = repeat(workflow_sha_byte, 40);
    let request_value = json!({
        "schema_version": 1,
        "challenge": repeat("9", 64),
        "reservation_id": RESERVATION_ID,
        "intent_id": INTENT_ID,
        "repository_id": REPOSITORY_ID,
        "repository_name": "owner-101/repo-101",
        "event_name": "push",
        "event_ref": "refs/heads/main",
        "tested_sha": repeat("a", 40),
        "run_id": run_id,
        "run_attempt": run_attempt,
        "job_key": "verify",
        "workflow_ref": format!(
            "owner-101/repo-101/{WORKFLOW_PATH}@refs/heads/main"
        ),
        "workflow_sha": workflow_sha,
        "base_ref": null,
        "base_sha": null,
        "head_ref": null,
        "head_sha": null,
        "pull_request_number": null
    });
    let request: ExecutionSourceRequest =
        serde_json::from_value(request_value.clone()).expect("valid hook request");
    let request_digest = request.sha256().expect("canonical request digest");
    let persisted: PersistedExecutionSource = serde_json::from_value(json!({
        "request": request_value,
        "request_digest": request_digest,
        "tested_commit_sha": repeat("a", 40),
        "root_workflow_path": WORKFLOW_PATH,
        "root_commit_sha": workflow_sha,
        "root_tree_sha": repeat("e", 40),
        "root_blob_sha": repeat("d", 40),
        "root_content_sha256": repeat("c", 64),
        "leaf_workflow_path": WORKFLOW_PATH,
        "leaf_commit_sha": workflow_sha,
        "leaf_tree_sha": repeat("e", 40),
        "leaf_blob_sha": repeat("d", 40),
        "leaf_content_sha256": repeat("c", 64),
        "workflow_closure": [{
            "repository_id": REPOSITORY_ID,
            "path": WORKFLOW_PATH,
            "commit_sha": workflow_sha,
            "tree_sha": repeat("e", 40),
            "blob_sha": repeat("d", 40),
            "content_sha256": repeat("c", 64)
        }],
        "pull_request_source": null,
        "rest_job_id": "rest-job-101"
    }))
    .expect("valid persisted source");
    test_authorized_execution_source(persisted, repeat("8", 64)).expect("valid live source wrapper")
}

fn started_execution(
    actual_request_id: i64,
    run_id: i64,
    run_attempt: i64,
    stored_workflow_sha_byte: Option<&str>,
) -> StartedExecution {
    let workflow_source_sha = stored_workflow_sha_byte.map(|byte| repeat(byte, 40));
    let source_json = serde_json::to_string(&StoredRequestSource {
        repository_id: REPOSITORY_ID,
        owner_name: "owner-101".to_owned(),
        repository_name: "repo-101".to_owned(),
        workflow_run_id: run_id,
        event_name: "push".to_owned(),
        head_branch: Some("main".to_owned()),
        head_sha: repeat("a", 40),
        job_workflow_ref: format!("owner-101/repo-101/{WORKFLOW_PATH}@refs/heads/main"),
        workflow: StoredRequestWorkflow {
            full_ref: format!("owner-101/repo-101/{WORKFLOW_PATH}@refs/heads/main"),
            owner_name: "owner-101".to_owned(),
            repository_name: "repo-101".to_owned(),
            workflow_path: WORKFLOW_PATH.to_owned(),
            reference: "refs/heads/main".to_owned(),
            kind: "branch".to_owned(),
        },
        workflow_source_sha,
        run_attempt,
    })
    .expect("canonical stored source");
    StartedExecution {
        request_id: actual_request_id,
        event_digest: repeat("a", 64),
        native_job_id: Some(format!("native-job-{actual_request_id}")),
        runner_id: RUNNER_ID,
        runner_name: runner_name(),
        jit_runner_id: Some(RUNNER_ID),
        jit_runner_name: Some(runner_name()),
        jit_started_request_id: Some(actual_request_id),
        jit_state: "started".to_owned(),
        jit_completed_request_id: None,
        jit_witness_request_id: WITNESS_REQUEST_ID,
        acquire_state: "acquired".to_owned(),
        completed_event_digest: None,
        state_delivery_state: "delivered".to_owned(),
        state_resources_bound: true,
        state_instance_binding_digest: Some(repeat("d", 64)),
        reservation_state: "reserved".to_owned(),
        runner_terminated: false,
        source_digest: digest(source_json.as_bytes()),
        source_json,
    }
}

fn proof_row() -> ProofRow {
    let challenge = repeat("9", 64);
    let policy = policy_snapshot().policy;
    let challenge_document = ExecutionProofChallenge::from_journal(ExecutionProofChallengeFields {
        challenge: challenge.clone(),
        reservation_id: RESERVATION_ID,
        intent_id: INTENT_ID,
        witness_request_id: WITNESS_REQUEST_ID,
        runner_id: RUNNER_ID,
        runner_name: runner_name(),
        runner_scale_set_id: SCALE_SET_ID,
        runner_container_id: repeat("1", 64),
        dind_container_id: repeat("2", 64),
        engine_id: ENGINE_ID.to_owned(),
        docker_root_digest: repeat("3", 64),
        instance_binding_digest: repeat("d", 64),
        policy_generation: 1,
        policy_digest: policy.policy_sha256().to_owned(),
        channel_digest: repeat("8", 64),
    })
    .expect("valid challenge");
    ProofRow {
        reservation_id: RESERVATION_ID,
        scale_set_id: SCALE_SET_ID,
        witness_request_id: WITNESS_REQUEST_ID,
        binding_digest: policy.binding_sha256().to_owned(),
        generation: 1,
        policy_digest: policy.policy_sha256().to_owned(),
        runner_id: RUNNER_ID,
        runner_name: runner_name(),
        instance_binding_digest: repeat("d", 64),
        challenge,
        challenge_state: "published".to_owned(),
        challenge_document,
        source_digest: None,
        source_json: None,
        proof_state: "pending".to_owned(),
    }
}

fn policy_snapshot() -> PolicySnapshot {
    let repository =
        crate::journal::request_lifecycle::tests::helpers::binding(CONNECTION_ID, REPOSITORY_ID)
            .expect("valid repository binding");
    let binding_digest = digest(
        &repository
            .canonical_identity_bytes()
            .expect("canonical binding"),
    );
    let mut policy_value = json!({
        "schema_version": 1,
        "connection_id": CONNECTION_ID,
        "binding_sha256": binding_digest,
        "generation": 1,
        "policy_sha256": repeat("0", 64),
        "generator": {"source_sha": repeat("a", 40), "artifact_sha256": repeat("b", 64)},
        "workflows": [{
            "repository_id": REPOSITORY_ID,
            "path": WORKFLOW_PATH,
            "content_sha256": repeat("c", 64),
            "calls": [],
            "jobs": [{
                "root_path": WORKFLOW_PATH,
                "workflow_path": WORKFLOW_PATH,
                "job_key": "verify",
                "role": "verification",
                "contexts": [{"event": "push", "refs": ["refs/heads/main"], "pull_request_rules": []}],
                "permissions_declared": true,
                "permissions": {"contents": "read"},
                "secrets": [],
                "environments": [],
                "condition_sha256": null,
                "concurrency": null,
                "needs": []
            }]
        }]
    });
    let mut policy: ApprovedJobAuthorityPolicy =
        serde_json::from_value(policy_value.clone()).expect("policy data");
    policy_value["policy_sha256"] =
        json!(digest(&policy.canonical_json().expect("canonical policy")));
    policy = serde_json::from_value(policy_value).expect("complete policy");
    policy
        .validate_for(&repository)
        .expect("policy matches the repository");
    PolicySnapshot {
        policy,
        repository,
        is_active: true,
    }
}

#[derive(Serialize)]
struct StoredRequestSource {
    repository_id: i64,
    owner_name: String,
    repository_name: String,
    workflow_run_id: i64,
    event_name: String,
    head_branch: Option<String>,
    head_sha: String,
    job_workflow_ref: String,
    workflow: StoredRequestWorkflow,
    workflow_source_sha: Option<String>,
    run_attempt: i64,
}

#[derive(Serialize)]
struct StoredRequestWorkflow {
    full_ref: String,
    owner_name: String,
    repository_name: String,
    workflow_path: String,
    reference: String,
    kind: String,
}

fn runner_name() -> String {
    format!("v{}", "n".repeat(32))
}

fn repeat(byte: &str, count: usize) -> String {
    byte.repeat(count)
}

fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}
