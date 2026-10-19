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

#[test]
fn pull_request_compares_api_head_with_hook_head_not_tested_merge_commit() -> TestResult<()> {
    let execution = execution_source(true, ROOT_PATH, "f", "f")?;
    let started = started_execution(true, ROOT_PATH, "f", "f", "b")?;

    assert!(started.matches_source_request(&execution)?);
    Ok(())
}

#[test]
fn pull_request_rejects_a_stored_merge_sha_as_the_head_sha() -> TestResult<()> {
    let execution = execution_source(true, ROOT_PATH, "f", "f")?;
    let started = started_execution(true, ROOT_PATH, "f", "f", "a")?;

    assert!(!started.matches_source_request(&execution)?);
    Ok(())
}

#[test]
fn non_pull_request_still_matches_the_tested_commit() -> TestResult<()> {
    let execution = execution_source(false, ROOT_PATH, "f", "f")?;
    let started = started_execution(false, ROOT_PATH, "f", "f", "b")?;

    assert!(!started.matches_source_request(&execution)?);
    Ok(())
}

#[test]
fn same_repository_reusable_workflow_matches_its_verified_leaf_commit() -> TestResult<()> {
    let execution = execution_source(true, LEAF_PATH, "f", "d")?;
    let started = started_execution(true, LEAF_PATH, "f", "d", "b")?;

    assert!(started.matches_source_request(&execution)?);
    Ok(())
}

#[test]
fn reusable_workflow_rejects_missing_or_wrong_leaf_commit() -> TestResult<()> {
    let execution = execution_source(true, LEAF_PATH, "f", "d")?;
    for stored_leaf_sha in [None, Some("e")] {
        let started = started_execution_with_leaf_sha(
            true,
            LEAF_PATH,
            "d",
            stored_leaf_sha.map(|value| repeat(value, 40)),
            "b",
        )?;
        assert!(!started.matches_source_request(&execution)?);
    }
    Ok(())
}

#[test]
fn reusable_workflow_rejects_wrong_leaf_path_and_root_only_claim() -> TestResult<()> {
    let reusable = execution_source(true, LEAF_PATH, "f", "d")?;
    let wrong_path = started_execution(true, ROOT_PATH, "f", "d", "b")?;
    assert!(!wrong_path.matches_source_request(&reusable)?);

    let root = execution_source(true, ROOT_PATH, "f", "f")?;
    let wrong_root = started_execution(true, ROOT_PATH, "e", "e", "b")?;
    assert!(!wrong_root.matches_source_request(&root)?);
    Ok(())
}

#[test]
fn native_ref_must_match_its_parsed_workflow_identity() -> TestResult<()> {
    let execution = execution_source(true, LEAF_PATH, "f", "d")?;
    let mut started = started_execution(true, LEAF_PATH, "f", "d", "b")?;
    let mut source: StoredRequestSource = serde_json::from_str(&started.source_json)?;
    source.workflow.full_ref = workflow_ref(LEAF_PATH, "refs/heads/other");
    started.source_json = serde_json::to_string(&source)?;
    started.source_digest = digest(started.source_json.as_bytes());

    assert!(started.matches_source_request(&execution).is_err());
    Ok(())
}

#[test]
fn native_reusable_workflow_cannot_cross_repository_boundary() -> TestResult<()> {
    let execution = execution_source(true, LEAF_PATH, "f", "d")?;
    let mut started = started_execution(true, LEAF_PATH, "f", "d", "b")?;
    let mut source: StoredRequestSource = serde_json::from_str(&started.source_json)?;
    source.job_workflow_ref = format!("other/repository/{LEAF_PATH}@refs/heads/reusable-v2");
    source.workflow.full_ref = source.job_workflow_ref.clone();
    source.workflow.owner_name = "other".to_owned();
    source.workflow.repository_name = "repository".to_owned();
    started.source_json = serde_json::to_string(&source)?;
    started.source_digest = digest(started.source_json.as_bytes());

    assert!(!started.matches_source_request(&execution)?);
    Ok(())
}

#[test]
fn native_context_requires_the_exact_started_runner_request() -> TestResult<()> {
    let request: ExecutionSourceRequest =
        serde_json::from_value(request_value(true, &repeat("f", 40)))?;
    let mut started = started_execution(true, ROOT_PATH, "f", "f", "b")?;
    started.jit_runner_id = Some(701);
    started.jit_runner_name = Some("runner-701".to_owned());
    started.jit_started_request_id = Some(102);
    let channel = test_channel_binding()?;

    let context =
        super::super::super::native_context::validate_started(&started, &request, &channel)?;
    assert_eq!(
        context.job_workflow_ref(),
        workflow_ref(ROOT_PATH, "refs/pull/9/merge")
    );
    assert_eq!(
        context.workflow_source_sha(),
        Some(repeat("f", 40).as_str())
    );

    let mut reusable = started_execution(true, LEAF_PATH, "f", "d", "b")?;
    reusable.jit_runner_id = Some(701);
    reusable.jit_runner_name = Some("runner-701".to_owned());
    reusable.jit_started_request_id = Some(102);
    let reusable_context =
        super::super::super::native_context::validate_started(&reusable, &request, &channel)?;
    assert_eq!(
        reusable_context.job_workflow_ref(),
        workflow_ref(LEAF_PATH, "refs/heads/reusable-v2")
    );
    assert_eq!(
        reusable_context.workflow_source_sha(),
        Some(repeat("d", 40).as_str())
    );

    let mut mismatched = started;
    mismatched.runner_id = 702;
    assert!(
        super::super::super::native_context::validate_started(&mismatched, &request, &channel,)
            .is_err()
    );
    Ok(())
}

fn test_channel_binding()
-> TestResult<crate::worker::execution_authorization::VerifiedProofChannelBinding> {
    use crate::worker::execution_authorization::{
        test_proof_channel_mount, test_verified_proof_channel_binding,
    };
    let digest = repeat("d", 64);
    let control = test_proof_channel_mount(
        "/opt/velnor/source-proof/control".to_owned(),
        "source-proof/control".to_owned(),
        1,
        (1, 2),
        (1, 2),
        123,
        1,
        digest.clone(),
        10,
        (0, 0),
        0o555,
        true,
        true,
    )?;
    let inbox = test_proof_channel_mount(
        "/opt/velnor/source-proof/inbox".to_owned(),
        "source-proof/inbox".to_owned(),
        2,
        (0, 43),
        (0, 43),
        0x0102_1994,
        2,
        repeat("f", 64),
        11,
        (1000, 1000),
        0o700,
        false,
        true,
    )?;
    Ok(test_verified_proof_channel_binding(
        11,
        12,
        "connection-1".to_owned(),
        13,
        100,
        "engine".to_owned(),
        repeat("e", 64),
        701,
        "runner-701".to_owned(),
        repeat("a", 12),
        repeat("b", 12),
        repeat("c", 64),
        "metadata".to_owned(),
        repeat("c", 12),
        control,
        inbox,
        true,
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
