use super::*;

use crate::launch::execution_authorization::{
    AuthorizedExecutionSource, ExecutionSourceRequest, PersistedExecutionSource,
    test_authorized_execution_source,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

const REPOSITORY_ID: i64 = 101;
const OWNER: &str = "owner-101";
const REPOSITORY: &str = "repo-101";
const ROOT_PATH: &str = ".github/workflows/build.yml";
const LEAF_PATH: &str = ".github/workflows/reusable.yml";
type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

fn test_channel_binding()
-> TestResult<crate::worker::execution_authorization::VerifiedProofChannelBinding> {
    use crate::worker::execution_authorization::{
        test_proof_channel_mount, test_verified_proof_channel_binding,
    };
    let digest = repeat("d", 64);
    let control = test_proof_channel_mount(
        crate::worker::execution_authorization::TestProofChannelMount {
            target: "/opt/velnor/source-proof/control".to_owned(),
            subpath: "source-proof/control".to_owned(),
            mount_id: 1,
            device: (1, 2),
            statx_device: (1, 2),
            fs_type_magic: 123,
            filesystem_id: 1,
            filesystem_instance_digest: digest.clone(),
            directory_inode: 10,
            owner: (0, 0),
            mode: 0o555,
            read_only: true,
            no_copy: true,
        },
    )?;
    let inbox = test_proof_channel_mount(
        crate::worker::execution_authorization::TestProofChannelMount {
            target: "/opt/velnor/source-proof/inbox".to_owned(),
            subpath: "source-proof/inbox".to_owned(),
            mount_id: 2,
            device: (0, 43),
            statx_device: (0, 43),
            fs_type_magic: 0x0102_1994,
            filesystem_id: 2,
            filesystem_instance_digest: repeat("f", 64),
            directory_inode: 11,
            owner: (1000, 1000),
            mode: 0o700,
            read_only: false,
            no_copy: true,
        },
    )?;
    Ok(test_verified_proof_channel_binding(
        crate::worker::execution_authorization::TestProofChannelBindingFields {
            identity: crate::worker::execution_authorization::TestProofChannelIdentity {
                reservation_id: 11,
                operation_id: 12,
                connection_id: "connection-1".to_owned(),
                scale_set_id: 13,
                witness_request_id: 100,
                engine_id: "engine".to_owned(),
                docker_root_digest: repeat("e", 64),
                runner_id: 701,
                runner_name: "runner-701".to_owned(),
                runner_container_id: repeat("a", 12),
                dind_container_id: repeat("b", 12),
                instance_binding_digest: repeat("c", 64),
            },
            metadata_volume_name: "metadata".to_owned(),
            metadata_volume_id: repeat("c", 12),
            control: control,
            inbox: inbox,
            dind_attachment_absent: true,
        },
    )?)
}

fn execution_source(
    pull_request: bool,
    leaf_path: &str,
    root_sha_byte: &str,
    leaf_sha_byte: &str,
) -> TestResult<AuthorizedExecutionSource> {
    let root_sha = repeat(root_sha_byte, 40);
    let leaf_sha = repeat(leaf_sha_byte, 40);
    let request_value = request_value(pull_request, &root_sha);
    let request: ExecutionSourceRequest = serde_json::from_value(request_value.clone())?;
    let request_digest = request.sha256()?;
    let leaf_record = json!({
        "repository_id": REPOSITORY_ID,
        "path": leaf_path,
        "commit_sha": leaf_sha,
        "tree_sha": repeat("e", 40),
        "blob_sha": repeat("d", 40),
        "content_sha256": repeat("c", 64)
    });
    let root_record = json!({
        "repository_id": REPOSITORY_ID,
        "path": ROOT_PATH,
        "commit_sha": root_sha,
        "tree_sha": repeat("e", 40),
        "blob_sha": repeat("d", 40),
        "content_sha256": repeat("b", 64)
    });
    let mut closure = vec![root_record];
    if leaf_path != ROOT_PATH {
        closure.push(leaf_record);
    }
    let pull_request_source = pull_request.then(|| {
        json!({
            "number": 9,
            "base_repository_id": REPOSITORY_ID,
            "base_repository_name": format!("{OWNER}/{REPOSITORY}"),
            "base_ref": "sea/integration",
            "base_sha": repeat("c", 40),
            "head_repository_id": REPOSITORY_ID,
            "head_repository_name": format!("{OWNER}/{REPOSITORY}"),
            "head_ref": "feature",
            "head_sha": repeat("b", 40),
            "merge_ref": "refs/pull/9/merge",
            "merge_commit_sha": repeat("a", 40)
        })
    });
    let persisted: PersistedExecutionSource = serde_json::from_value(json!({
        "request": request_value,
        "request_digest": request_digest,
        "tested_commit_sha": repeat("a", 40),
        "root_workflow_path": ROOT_PATH,
        "root_commit_sha": root_sha,
        "root_tree_sha": repeat("e", 40),
        "root_blob_sha": repeat("d", 40),
        "root_content_sha256": repeat("b", 64),
        "leaf_workflow_path": leaf_path,
        "leaf_commit_sha": leaf_sha,
        "leaf_tree_sha": repeat("e", 40),
        "leaf_blob_sha": repeat("d", 40),
        "leaf_content_sha256": if leaf_path == ROOT_PATH { repeat("b", 64) } else { repeat("c", 64) },
        "workflow_closure": closure,
        "pull_request_source": pull_request_source,
        "rest_job_id": "rest-job-101"
    }))?;
    Ok(test_authorized_execution_source(
        persisted,
        repeat("8", 64),
    )?)
}

fn request_value(pull_request: bool, workflow_sha: &str) -> serde_json::Value {
    json!({
        "schema_version": 1,
        "challenge": repeat("9", 64),
        "reservation_id": 11,
        "intent_id": 12,
        "repository_id": REPOSITORY_ID,
        "repository_name": format!("{OWNER}/{REPOSITORY}"),
        "event_name": if pull_request { "pull_request" } else { "push" },
        "event_ref": if pull_request { "refs/pull/9/merge" } else { "refs/heads/main" },
        "tested_sha": repeat("a", 40),
        "run_id": 101,
        "run_attempt": 1,
        "job_key": "verify",
        "workflow_ref": workflow_ref(ROOT_PATH, if pull_request { "refs/pull/9/merge" } else { "refs/heads/main" }),
        "workflow_sha": workflow_sha,
        "base_ref": if pull_request { Some("sea/integration") } else { None },
        "base_sha": if pull_request { Some(repeat("c", 40)) } else { None },
        "head_ref": if pull_request { Some("feature") } else { None },
        "head_sha": if pull_request { Some(repeat("b", 40)) } else { None },
        "pull_request_number": if pull_request { Some(9) } else { None }
    })
}

fn started_execution(
    pull_request: bool,
    native_path: &str,
    root_sha_byte: &str,
    leaf_sha_byte: &str,
    stored_head_byte: &str,
) -> TestResult<StartedExecution> {
    started_execution_with_leaf_sha(
        pull_request,
        native_path,
        root_sha_byte,
        Some(repeat(leaf_sha_byte, 40)),
        stored_head_byte,
    )
}

fn started_execution_with_leaf_sha(
    pull_request: bool,
    native_path: &str,
    root_sha_byte: &str,
    leaf_sha: Option<String>,
    stored_head_byte: &str,
) -> TestResult<StartedExecution> {
    let root_reference = if pull_request {
        "refs/pull/9/merge"
    } else {
        "refs/heads/main"
    };
    let native_reference = if native_path == ROOT_PATH {
        root_reference
    } else {
        "refs/heads/reusable-v2"
    };
    let source_json = serde_json::to_string(&StoredRequestSource {
        repository_id: REPOSITORY_ID,
        owner_name: OWNER.to_owned(),
        repository_name: REPOSITORY.to_owned(),
        workflow_run_id: 101,
        event_name: if pull_request { "pull_request" } else { "push" }.to_owned(),
        head_branch: Some(if pull_request { "feature" } else { "main" }.to_owned()),
        head_sha: repeat(stored_head_byte, 40),
        job_workflow_ref: workflow_ref(native_path, native_reference),
        workflow: StoredRequestWorkflow {
            full_ref: workflow_ref(native_path, native_reference),
            owner_name: OWNER.to_owned(),
            repository_name: REPOSITORY.to_owned(),
            workflow_path: native_path.to_owned(),
            reference: native_reference.to_owned(),
            kind: if pull_request && native_path == ROOT_PATH {
                "pull_request"
            } else {
                "branch"
            }
            .to_owned(),
        },
        workflow_source_sha: leaf_sha
            .or_else(|| (native_path == ROOT_PATH).then(|| repeat(root_sha_byte, 40))),
        run_attempt: 1,
    })?;
    let digest = digest(source_json.as_bytes());
    Ok(StartedExecution {
        request_id: 102,
        event_digest: repeat("a", 64),
        native_job_id: Some("native-job-102".to_owned()),
        runner_id: 701,
        runner_name: "runner-701".to_owned(),
        jit_runner_id: None,
        jit_runner_name: None,
        jit_started_request_id: None,
        jit_state: "started".to_owned(),
        jit_completed_request_id: None,
        jit_witness_request_id: 100,
        acquire_state: "acquired".to_owned(),
        completed_event_digest: None,
        state_delivery_state: "delivered".to_owned(),
        state_resources_bound: true,
        state_instance_binding_digest: Some(repeat("d", 64)),
        reservation_state: "reserved".to_owned(),
        runner_terminated: false,
        source_digest: digest,
        source_json,
    })
}

fn workflow_ref(path: &str, reference: &str) -> String {
    format!("{OWNER}/{REPOSITORY}/{path}@{reference}")
}

fn repeat(value: &str, length: usize) -> String {
    value.repeat(length)
}

#[derive(Deserialize, Serialize)]
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

#[derive(Deserialize, Serialize)]
struct StoredRequestWorkflow {
    full_ref: String,
    owner_name: String,
    repository_name: String,
    workflow_path: String,
    reference: String,
    kind: String,
}

#[path = "started_tests/cases.rs"]
mod cases;
