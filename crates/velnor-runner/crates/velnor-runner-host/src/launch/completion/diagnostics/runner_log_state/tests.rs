use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use super::{
    CaptureBinding, CapturePhase, CaptureState, CaptureStateRequest, binding as make_binding,
    read_projection, read_state, recover_terminal_projection,
};
use crate::launch::completion::diagnostics::DiagnosticsRoot;

#[path = "tests/storage.rs"]
mod storage;

fn binding() -> CaptureBinding {
    CaptureBinding {
        journal_identity: "/private/journal".to_owned(),
        connection_id: "connection-1".to_owned(),
        intent_id: 11,
        reservation_id: 22,
        operation_id: 33,
        topology_version: 1,
        engine_id: "engine-1".to_owned(),
        docker_root_digest: "a".repeat(64),
        runner_container_id: "b".repeat(64),
        dind_container_id: "c".repeat(64),
        runner_home_volume: "runner-home-22".to_owned(),
    }
}

#[test]
fn uncertain_start_reuses_only_the_same_exec_then_retries_after_terminal() {
    let binding = binding();
    let prepared = CaptureState::prepared(binding.clone(), 1);
    let create_requested = prepared
        .next_create_request()
        .expect("The test cannot prepare the create request.");
    let created = create_requested
        .created("d".repeat(64))
        .expect("The test cannot record the created execution identity.");
    let first_start = created
        .next_start_request()
        .expect("The test cannot prepare the first start request.");
    let retry_start = first_start
        .next_start_request()
        .expect("The test cannot prepare a retry for the same execution ID.");

    assert_eq!(first_start.exec_id, retry_start.exec_id);
    assert_eq!(retry_start.phase, CapturePhase::StartRequested);
    assert_eq!(retry_start.start_requests, 2);

    let terminal = retry_start
        .terminal(0)
        .expect("The test cannot record the final execution result.");
    let replay = terminal
        .retry_after_terminal()
        .expect("The test cannot retry capture after the final execution result.");
    assert_eq!(replay.binding, binding);
    assert_eq!(replay.attempt, 2);
    assert_eq!(replay.phase, CapturePhase::Prepared);
    assert_eq!(replay.exec_id, None);
}

#[test]
fn exit_without_code_cannot_become_terminal_or_authorize_retry() {
    let binding = binding();
    let state = CaptureState::prepared(binding, 1)
        .next_create_request()
        .expect("The test cannot prepare the create request.")
        .created("d".repeat(64))
        .expect("The test cannot record the created execution identity.")
        .next_start_request()
        .expect("The test cannot prepare the start request.");

    assert!(state.terminal(-1).is_err());
    assert!(state.retry_after_terminal().is_err());
}

#[test]
fn projection_attempts_remain_bounded_across_reopens() -> Result<(), String> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let (state_path, binding) = attempt_fixture(&NEXT)?;
    let mut root = DiagnosticsRoot::open(&state_path, &binding.connection_id)
        .map_err(|error| format!("{error:?}"))?;
    let mut state = CaptureState::prepared(binding.clone(), 1);
    persist_state(&root, &binding, &state)?;

    for attempt in 1..=3 {
        state = state
            .next_create_request()
            .map_err(|error| format!("{error:?}"))?;
        persist_state(&root, &binding, &state)?;
        state = state
            .created(format!("{attempt:064x}"))
            .map_err(|error| format!("{error:?}"))?;
        persist_state(&root, &binding, &state)?;
        state = state
            .next_start_request()
            .map_err(|error| format!("{error:?}"))?;
        persist_state(&root, &binding, &state)?;
        state = state.terminal(0).map_err(|error| format!("{error:?}"))?;
        persist_state(&root, &binding, &state)?;
        state = state
            .projection_pending([u8::try_from(attempt).unwrap_or(0); 32], 1, true)
            .map_err(|error| format!("{error:?}"))?;
        persist_state(&root, &binding, &state)?;

        drop(root);
        root = DiagnosticsRoot::open(&state_path, &binding.connection_id)
            .map_err(|error| format!("{error:?}"))?;
        state = read_state(&root, &binding)
            .map_err(|error| format!("{error:?}"))?
            .ok_or_else(|| "The saved pending record state is absent.".to_owned())?;
        if recover_terminal_projection(&root, &binding, &state)
            .map_err(|error| format!("{error:?}"))?
            .is_some()
        {
            return Err("The recovery check accepted an absent record.".to_owned());
        }
        if attempt < 3 {
            state = state
                .retry_after_terminal()
                .map_err(|error| format!("{error:?}"))?;
            if state.attempt != attempt + 1 {
                return Err("The retry did not increase the record attempt number.".to_owned());
            }
            persist_state(&root, &binding, &state)?;
        } else if state.retry_after_terminal().is_ok() {
            return Err(
                "The state check allowed more record attempts than the configured limit."
                    .to_owned(),
            );
        }
    }

    drop(root);
    fs::remove_dir_all(state_path).map_err(|error| error.to_string())
}

fn attempt_fixture(
    counter: &std::sync::atomic::AtomicU64,
) -> Result<(std::path::PathBuf, CaptureBinding), String> {
    let id = counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "velnor-runner-log-attempts-{}-{id}",
        std::process::id()
    ));
    fs::create_dir(&path).map_err(|error| error.to_string())?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    let path = fs::canonicalize(path).map_err(|error| error.to_string())?;
    let root = DiagnosticsRoot::open(&path, "attempt-reopen-connection")
        .map_err(|error| format!("{error:?}"))?;
    let binding = make_binding(
        &root,
        12,
        23,
        34,
        1,
        "engine-1",
        &"a".repeat(64),
        &"b".repeat(64),
        &"c".repeat(64),
        "runner-home-23",
    )
    .map_err(|error| format!("{error:?}"))?;
    Ok((path, binding))
}

#[test]
fn terminal_projection_is_recovered_after_restart_before_completion_state_write()
-> Result<(), String> {
    let (state_path, root, binding, terminal) = recovery_fixture()?;
    let (pending, request) = nonempty_b_projection(&root, &binding, &terminal)?;
    persist_state(&root, &binding, &pending)?;
    if recover_terminal_projection(&root, &binding, &pending)
        .map_err(|error| format!("{error:?}"))?
        .is_some()
    {
        return Err("The recovery check accepted an absent record.".to_owned());
    }
    request
        .write_request(&AtomicBool::new(false))
        .map_err(|error| format!("{error:?}"))?;
    if recover_terminal_projection(&root, &binding, &terminal).is_ok() {
        return Err(
            "The recovery check accepted a record without the saved digest and size.".to_owned(),
        );
    }

    drop(root);
    let reopened = DiagnosticsRoot::open(&state_path, &binding.connection_id)
        .map_err(|error| format!("{error:?}"))?;
    let terminal = read_state(&reopened, &binding)
        .map_err(|error| format!("{error:?}"))?
        .ok_or_else(|| "The saved pending record state is absent or invalid.".to_owned())?;
    if terminal.phase != CapturePhase::ProjectionPending {
        return Err("The saved pending record state is absent or invalid.".to_owned());
    }
    let mut substituted = terminal.clone();
    substituted.projection_bytes = substituted.projection_bytes.map(|size| size + 1);
    if persist_state(&reopened, &binding, &substituted).is_ok() {
        return Err(
            "The state check allowed a change to the expected record digest or size.".to_owned(),
        );
    }
    let mut mismatched = terminal.clone();
    mismatched.projection_sha256 = Some("0".repeat(64));
    if recover_terminal_projection(&reopened, &binding, &mismatched).is_ok() {
        return Err(
            "The recovery check accepted a record with the wrong expected digest.".to_owned(),
        );
    }
    let recovered = recover_terminal_projection(&reopened, &binding, &terminal)
        .map_err(|error| format!("{error:?}"))?
        .ok_or_else(|| "The recovery check did not restore the saved record.".to_owned())?;
    if !recovered.0 {
        return Err("The recovered record does not show that trace data is present.".to_owned());
    }
    let complete = terminal.projected().map_err(|error| format!("{error:?}"))?;
    persist_state(&reopened, &binding, &complete)?;
    let completed = read_state(&reopened, &binding)
        .map_err(|error| format!("{error:?}"))?
        .ok_or_else(|| "The saved complete state is absent.".to_owned())?;
    if completed.phase != CapturePhase::Complete {
        return Err(
            "The recovered record did not reach the complete state with the expected bytes."
                .to_owned(),
        );
    }
    let bytes =
        read_projection(&reopened, &binding, &completed).map_err(|error| format!("{error:?}"))?;
    let projection: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if projection["events"][0]["timestamp"] != "2026-10-21 12:34:56Z" {
        return Err("The recovered record changed the Runner trace timestamp.".to_owned());
    }
    fs::remove_dir_all(state_path).map_err(|error| error.to_string())
}

fn recovery_fixture() -> Result<
    (
        std::path::PathBuf,
        DiagnosticsRoot,
        CaptureBinding,
        CaptureState,
    ),
    String,
> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "velnor-runner-log-state-recovery-{}-{id}",
        std::process::id()
    ));
    fs::create_dir(&path).map_err(|error| error.to_string())?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    let path = fs::canonicalize(path).map_err(|error| error.to_string())?;
    let root = DiagnosticsRoot::open(&path, "state-recovery-connection")
        .map_err(|error| format!("{error:?}"))?;
    let binding = make_binding(
        &root,
        11,
        22,
        33,
        1,
        "engine-1",
        &"a".repeat(64),
        &"b".repeat(64),
        &"c".repeat(64),
        "runner-home-22",
    )
    .map_err(|error| format!("{error:?}"))?;
    let terminal = terminal_state(&root, &binding)?;
    Ok((path, root, binding, terminal))
}

fn terminal_state(
    root: &DiagnosticsRoot,
    binding: &CaptureBinding,
) -> Result<CaptureState, String> {
    let mut state = CaptureState::prepared(binding.clone(), 1);
    persist_state(root, binding, &state)?;
    state = state
        .next_create_request()
        .map_err(|error| format!("{error:?}"))?;
    persist_state(root, binding, &state)?;
    state = state
        .created("d".repeat(64))
        .map_err(|error| format!("{error:?}"))?;
    persist_state(root, binding, &state)?;
    state = state
        .next_start_request()
        .map_err(|error| format!("{error:?}"))?;
    persist_state(root, binding, &state)?;
    state = state.terminal(0).map_err(|error| format!("{error:?}"))?;
    persist_state(root, binding, &state)?;
    Ok(state)
}

fn nonempty_b_projection(
    root: &DiagnosticsRoot,
    binding: &CaptureBinding,
    terminal: &CaptureState,
) -> Result<
    (
        CaptureState,
        super::super::runner_logs::RunnerLogStageRequest,
    ),
    String,
> {
    let key = super::super::stage::OperationKey {
        journal_identity: binding.journal_identity.clone(),
        connection_id: binding.connection_id.clone(),
        intent_id: binding.intent_id,
    };
    let archive = runner_trace_archive()?;
    let request = super::super::runner_logs::RunnerLogStageRequest::new(
        root,
        key,
        terminal.attempt,
        &binding.runner_container_id,
        &archive,
        Instant::now(),
    )
    .map_err(|error| format!("{error:?}"))?;
    let availability = request.availability();
    if availability != super::super::runner_logs::RunnerLogAvailability::RunnerOnly {
        return Err("The collector did not produce the expected Runner trace record.".to_owned());
    }
    let (digest, size) = request.projection_identity();
    let pending = terminal
        .projection_pending(digest, size, true)
        .map_err(|error| format!("{error:?}"))?;
    Ok((pending, request))
}

fn runner_trace_archive() -> Result<Vec<u8>, String> {
    let body = b"[2026-10-21 12:34:56Z INFO Runner.Worker] safe startup\n";
    let mut header = tar::Header::new_gnu();
    header.set_size(body.len() as u64);
    header.set_mode(0o600);
    header
        .set_path("Runner_20261021-123456-utc.log")
        .map_err(|error| error.to_string())?;
    header.set_cksum();
    let mut builder = tar::Builder::new(Vec::new());
    builder
        .append(&header, &body[..])
        .map_err(|error| error.to_string())?;
    builder.into_inner().map_err(|error| error.to_string())
}

fn test_root_path(label: &str) -> Result<std::path::PathBuf, String> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "velnor-runner-log-{label}-{}-{id}",
        std::process::id()
    ));
    fs::create_dir(&path).map_err(|error| error.to_string())?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    fs::canonicalize(path).map_err(|error| error.to_string())
}

fn diagnostics_scope(root: &std::path::Path, connection_id: &str) -> std::path::PathBuf {
    root.join("completion-diagnostics")
        .join(super::super::scope_component(connection_id))
}

fn persist_state(
    root: &DiagnosticsRoot,
    binding: &CaptureBinding,
    state: &CaptureState,
) -> Result<(), String> {
    CaptureStateRequest::new(root, binding, state, Instant::now())
        .and_then(|request| request.write_request(&AtomicBool::new(false)))
        .map_err(|error| format!("{error:?}"))
}
