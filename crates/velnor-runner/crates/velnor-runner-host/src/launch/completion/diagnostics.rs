use std::os::fd::OwnedFd;
use std::path::{Component, Path};
#[cfg(test)]
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use rustix::fs::{Mode, OFlags, open, openat};
use sha2::{Digest, Sha256};

#[path = "diagnostics/stage.rs"]
mod stage;
#[path = "diagnostics/worker.rs"]
mod worker;
use crate::error::HostError;
#[cfg(test)]
use stage::{OperationKey, StageRequest};

pub(super) const MAX_RECORD_BYTES: usize = 4_096;
pub(super) const DIRECTORY_MODE: u16 = 0o700;
pub(super) const FILE_MODE: u16 = 0o600;
pub(super) const SCOPE_DIRECTORY: &str = "completion-diagnostics";
const CAPTURE_DEADLINE: Duration = Duration::from_secs(15);
const INSPECT_DEADLINE: Duration = Duration::from_secs(10);

#[cfg(test)]
#[path = "diagnostics_tests.rs"]
mod tests;

pub(in crate::launch) struct DiagnosticsRoot {
    state: OwnedFd,
    parent: OwnedFd,
    scope: OwnedFd,
    journal_identity: String,
    connection_id: String,
    #[cfg(test)]
    next_write_gate: std::sync::Mutex<Option<Arc<std::sync::Barrier>>>,
}

#[cfg(test)]
struct OutputDirectories {
    state: OwnedFd,
    parent: OwnedFd,
    scope: OwnedFd,
    journal_identity: String,
    connection_id: String,
}

impl DiagnosticsRoot {
    #[cfg(test)]
    pub(super) fn open(state: &Path, connection_id: &str) -> Result<Self, HostError> {
        let journal_identity = state.to_string_lossy().into_owned();
        Self::open_for_journal(state, journal_identity, connection_id)
    }

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
            #[cfg(test)]
            next_write_gate: std::sync::Mutex::new(None),
        })
    }

    pub(super) fn pending_for(&self, intent_id: i64) -> Result<bool, HostError> {
        let worker = worker::process_worker()?;
        worker.pending_for(&self.journal_identity, &self.connection_id, intent_id)
    }

    #[cfg(test)]
    pub(in crate::launch) fn pause_next_write_for_test(
        &self,
    ) -> Result<Arc<std::sync::Barrier>, HostError> {
        let gate = Arc::new(std::sync::Barrier::new(2));
        let mut next = self.next_write_gate.lock().map_err(|_| HostError::Lock)?;
        if next.is_some() {
            return Err(HostError::Journal);
        }
        *next = Some(Arc::clone(&gate));
        Ok(gate)
    }

    #[cfg(test)]
    fn take_next_write_gate(&self) -> Result<Option<Arc<std::sync::Barrier>>, HostError> {
        self.next_write_gate
            .lock()
            .map(|mut next| next.take())
            .map_err(|_| HostError::Lock)
    }

    #[cfg(test)]
    pub(in crate::launch) fn hold_pending_write_for_test(
        &self,
        intent_id: i64,
        generation: i64,
    ) -> Result<PendingWrite, HostError> {
        if intent_id <= 0 || generation <= 0 {
            return Err(HostError::Ownership);
        }
        let gate = Arc::new(std::sync::Barrier::new(2));
        let request = StageRequest {
            state: self.state.try_clone().map_err(|_| HostError::Path)?,
            parent: self.parent.try_clone().map_err(|_| HostError::Path)?,
            scope: self.scope.try_clone().map_err(|_| HostError::Path)?,
            key: OperationKey {
                journal_identity: self.journal_identity.clone(),
                connection_id: self.connection_id.clone(),
                intent_id,
            },
            generation,
            role: "runner".to_owned(),
            container_id: "test-pending-container".to_owned(),
            bytes: b"test-only pending diagnostics record".to_vec(),
            started: Instant::now(),
        };
        let reply = worker::process_worker()?.submit_gated(
            request,
            Arc::new(AtomicBool::new(false)),
            Arc::clone(&gate),
        )?;
        Ok(PendingWrite {
            gate,
            reply: Some(reply),
            released: false,
        })
    }

    #[cfg(test)]
    fn output_directories(&self, started: Instant) -> Result<OutputDirectories, HostError> {
        check_deadline(started)?;
        Ok(OutputDirectories {
            state: self.state.try_clone().map_err(|_| HostError::Path)?,
            parent: self.parent.try_clone().map_err(|_| HostError::Path)?,
            scope: self.scope.try_clone().map_err(|_| HostError::Path)?,
            journal_identity: self.journal_identity.clone(),
            connection_id: self.connection_id.clone(),
        })
    }

    #[cfg(test)]
    fn file_name(identity: &CaptureIdentity<'_>) -> Result<String, HostError> {
        if identity.intent_id <= 0
            || identity.generation <= 0
            || !valid_role(identity.role)
            || identity.container_id.is_empty()
        {
            return Err(HostError::Ownership);
        }
        let digest = Sha256::digest(identity.container_id.as_bytes());
        Ok(format!(
            "i{}-g{}-{}-{}.json",
            identity.intent_id,
            identity.generation,
            identity.role,
            hex(&digest)
        ))
    }

    #[cfg(test)]
    fn write_record(
        directories: &OutputDirectories,
        identity: &CaptureIdentity<'_>,
        record: &DiagnosticRecord,
        started: Instant,
    ) -> Result<(), HostError> {
        check_deadline(started)?;
        if record.connection_id != identity.connection_id
            || record.intent_id != identity.intent_id
            || record.claim_generation != identity.generation
            || record.role != identity.role
            || record.container_id != identity.container_id
            || record.name != identity.expected_name
        {
            return Err(HostError::Ownership);
        }
        let bytes = serde_json::to_vec(record).map_err(|_| HostError::Journal)?;
        if bytes.len() > MAX_RECORD_BYTES {
            return Err(HostError::Ownership);
        }
        stage::write_request(
            StageRequest {
                state: directories.state.try_clone().map_err(|_| HostError::Path)?,
                parent: directories
                    .parent
                    .try_clone()
                    .map_err(|_| HostError::Path)?,
                scope: directories.scope.try_clone().map_err(|_| HostError::Path)?,
                key: OperationKey {
                    journal_identity: directories.journal_identity.clone(),
                    connection_id: directories.connection_id.clone(),
                    intent_id: identity.intent_id,
                },
                generation: identity.generation,
                role: identity.role.to_owned(),
                container_id: identity.container_id.to_owned(),
                bytes,
                started,
            },
            &AtomicBool::new(false),
        )
    }
}

#[cfg(test)]
pub(in crate::launch) struct PendingWrite {
    gate: Arc<std::sync::Barrier>,
    reply: Option<tokio::sync::oneshot::Receiver<Result<(), HostError>>>,
    released: bool,
}

#[cfg(test)]
impl PendingWrite {
    pub(in crate::launch) async fn finish(mut self) -> Result<(), HostError> {
        self.released = true;
        self.gate.wait();
        self.reply
            .take()
            .ok_or(HostError::Journal)?
            .await
            .map_err(|_| HostError::Journal)?
    }
}

#[cfg(test)]
impl Drop for PendingWrite {
    fn drop(&mut self) {
        if !self.released {
            self.gate.wait();
            self.released = true;
        }
    }
}

#[path = "diagnostics/record.rs"]
mod record;
#[cfg(test)]
pub(crate) use record::{CaptureIdentity, DiagnosticState, diagnostic_record};
pub(crate) use record::{DiagnosticRecord, RecordIdentity, diagnostic_record_fields};

#[path = "diagnostics/capture.rs"]
mod capture;
pub(in crate::launch) use capture::{NativeCaptureRequest, capture_native, delete_after_capture};

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

#[cfg(test)]
fn valid_role(role: &str) -> bool {
    matches!(role, "runner" | "dind")
}
