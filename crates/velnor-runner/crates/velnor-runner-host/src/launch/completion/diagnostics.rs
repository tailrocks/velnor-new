// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::fd::OwnedFd;
use std::path::{Component, Path};

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use rustix::fs::{Mode, OFlags, open, openat};
use sha2::{Digest, Sha256};

#[path = "diagnostics/runner_log_state.rs"]
mod runner_log_state;
#[path = "diagnostics/runner_logs.rs"]
mod runner_logs;
#[path = "diagnostics/stage.rs"]
mod stage;
#[path = "diagnostics/worker.rs"]
mod worker;
use crate::error::HostError;

pub(super) const MAX_RECORD_BYTES: usize = 4_096;
pub(super) const DIRECTORY_MODE: u16 = 0o700;
pub(super) const FILE_MODE: u16 = 0o600;
pub(super) const SCOPE_DIRECTORY: &str = "completion-diagnostics";
const CAPTURE_DEADLINE: Duration = Duration::from_secs(15);
const INSPECT_DEADLINE: Duration = Duration::from_secs(10);

pub(in crate::launch) struct DiagnosticsRoot {
    state: OwnedFd,
    parent: OwnedFd,
    scope: OwnedFd,
    journal_identity: String,
    connection_id: String,
}

impl DiagnosticsRoot {
    pub(super) fn open_for_journal(
        state: &Path,
        journal_identity: String,
        connection_id: &str,
    ) -> Result<Self, HostError> {
        if connection_id.is_empty() || matches!(connection_id, "." | "..") {
            return Err(HostError::Ownership);
        }
        let canonical = std::fs::canonicalize(state).map_err(|_| HostError::Path)?;
        if !state.is_absolute() || canonical != state {
            return Err(HostError::Path);
        }
        let mut current = open(
            "/",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| HostError::Path)?;
        for component in canonical.components() {
            let Component::Normal(name) = component else {
                continue;
            };
            current = openat(
                &current,
                name,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|_| HostError::Path)?;
        }
        stage::validate_private_directory(&current)?;
        let started = Instant::now();
        let deadline = started
            .checked_add(CAPTURE_DEADLINE)
            .ok_or(HostError::DockerTimeout)?;
        let cancelled = AtomicBool::new(false);
        let parent =
            stage::open_private_child(&current, SCOPE_DIRECTORY, started, deadline, &cancelled)?;
        let scope = stage::open_private_child(
            &parent,
            &scope_component(connection_id),
            started,
            deadline,
            &cancelled,
        )?;
        Ok(Self {
            state: current,
            parent,
            scope,
            journal_identity,
            connection_id: connection_id.to_owned(),
        })
    }

    pub(super) fn pending_for(&self, intent_id: i64) -> Result<bool, HostError> {
        let worker = worker::process_worker()?;
        worker.pending_for(&self.journal_identity, &self.connection_id, intent_id)
    }
}

#[path = "diagnostics/record.rs"]
mod record;

pub(crate) use record::{DiagnosticRecord, RecordIdentity, diagnostic_record_fields};

#[path = "diagnostics/capture.rs"]
mod capture;
pub(in crate::launch) use capture::{
    NativeCaptureRequest, RunnerLogWriteContext, capture_native, delete_after_capture,
    persist_runner_log_projection, persist_runner_log_state,
};
pub use runner_log_state::CapturePhase;
pub(in crate::launch) use runner_log_state::{
    CaptureBinding, CaptureBindingInput, CaptureState, binding as runner_log_binding,
    read_projection as read_runner_log_projection, read_state as read_runner_log_state,
    recover_terminal_projection as recover_runner_log_projection,
};

pub fn verify_runner_log_capture_completion(
    state_dir: &Path,
    exit_code: i64,
    archive: Vec<u8>,
) -> Result<CapturePhase, HostError> {
    let root = DiagnosticsRoot::open_for_journal(state_dir, "journal-1".to_owned(), "conn-1")?;
    let runner_id = "1".repeat(64);
    let dind_id = "2".repeat(64);
    let exec_id = "3".repeat(64);
    let binding = runner_log_binding(
        &root,
        &CaptureBindingInput {
            intent_id: 1,
            reservation_id: 2,
            operation_id: 3,
            topology_version: 1,
            engine_id: "engine-1",
            docker_root_digest: &"4".repeat(64),
            runner_container_id: &runner_id,
            dind_container_id: &dind_id,
            runner_home_volume: "velnor-home-1",
        },
    )?;
    let inspect = bollard::models::ExecInspectResponse {
        id: Some(exec_id.clone()),
        container_id: Some(dind_id),
        running: Some(false),
        exit_code: Some(exit_code),
        ..Default::default()
    };
    let checked = validate_runner_log_exec_exit(&exec_id, &inspect, archive)?;
    let started = Instant::now();
    let cancelled = AtomicBool::new(false);
    let prepared = CaptureState::prepared(binding.clone(), 1);
    runner_log_state::CaptureStateRequest::new(&root, &binding, &prepared, started)?
        .write_request(&cancelled)?;
    let create_req = prepared.next_create_request()?;
    runner_log_state::CaptureStateRequest::new(&root, &binding, &create_req, started)?
        .write_request(&cancelled)?;
    let created = create_req.created(exec_id)?;
    runner_log_state::CaptureStateRequest::new(&root, &binding, &created, started)?
        .write_request(&cancelled)?;
    let start_req = created.next_start_request()?;
    runner_log_state::CaptureStateRequest::new(&root, &binding, &start_req, started)?
        .write_request(&cancelled)?;
    let terminal = start_req.terminal(exit_code)?;
    runner_log_state::CaptureStateRequest::new(&root, &binding, &terminal, started)?
        .write_request(&cancelled)?;
    let stage = runner_logs::RunnerLogStageRequest::new(
        &root,
        stage::OperationKey {
            journal_identity: binding.journal_identity.clone(),
            connection_id: binding.connection_id.clone(),
            intent_id: binding.intent_id,
        },
        terminal.attempt,
        &binding.runner_container_id,
        &checked,
        started,
    )?;
    let has_trace = stage.availability() != runner_logs::RunnerLogAvailability::None;
    let (digest, size) = stage.projection_identity();
    let pending = terminal.projection_pending(digest, size, has_trace)?;
    runner_log_state::CaptureStateRequest::new(&root, &binding, &pending, started)?
        .write_request(&cancelled)?;
    stage.write_request(&cancelled)?;
    let projected = pending.projected()?;
    runner_log_state::CaptureStateRequest::new(&root, &binding, &projected, started)?
        .write_request(&cancelled)?;
    super::cleanup::admit_terminal_capture_phase(projected.phase)
        .map_err(|failure| failure.error)?;
    let replayed = read_runner_log_state(&root, &binding)?.ok_or(HostError::Ownership)?;
    read_runner_log_projection(&root, &binding, &replayed)?;
    super::cleanup::admit_terminal_capture_phase(replayed.phase)
        .map_err(|failure| failure.error)?;
    Ok(replayed.phase)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::launch) enum RunnerLogAvailability {
    None,
    RunnerOnly,
    WorkerOnly,
    RunnerAndWorker,
}

pub(in crate::launch) fn runner_log_create_exec_options() -> bollard::exec::CreateExecOptions<String>
{
    runner_logs::create_exec_options()
}

pub(in crate::launch) fn runner_log_start_exec_options() -> bollard::exec::StartExecOptions {
    runner_logs::start_exec_options()
}

pub(in crate::launch) fn validate_runner_log_exec_id(exec_id: &str) -> Result<(), HostError> {
    runner_logs::validate_exec_id(exec_id)
}

pub(in crate::launch) async fn collect_runner_log_exec_output(
    results: bollard::exec::StartExecResults,
    started: Instant,
    cancelled: &AtomicBool,
) -> Result<Vec<u8>, HostError> {
    runner_logs::collect_exec_output(results, started, cancelled).await
}

pub(in crate::launch) fn validate_runner_log_exec_exit(
    exec_id: &str,
    inspect: &bollard::models::ExecInspectResponse,
    archive: Vec<u8>,
) -> Result<Vec<u8>, HostError> {
    runner_logs::validate_exec_exit(exec_id, inspect, archive)
}

fn check_deadline(started: Instant) -> Result<(), HostError> {
    if started.elapsed() >= CAPTURE_DEADLINE {
        Err(HostError::DockerTimeout)
    } else {
        Ok(())
    }
}

fn check_capture_state(
    started: Instant,
    deadline: Instant,
    cancelled: &AtomicBool,
) -> Result<(), HostError> {
    if cancelled.load(Ordering::Acquire) || Instant::now() >= deadline {
        return Err(HostError::DockerTimeout);
    }
    check_deadline(started)
}

fn bounded_text(value: Option<String>, max_bytes: usize) -> Option<String> {
    value.filter(|text| text.len() <= max_bytes && !text.chars().any(char::is_control))
}

pub(super) fn scope_component(connection_id: &str) -> String {
    let digest = Sha256::digest(connection_id.as_bytes());
    hex(&digest)
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(char::from(DIGITS[usize::from(byte >> 4)]));
        value.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    value
}
