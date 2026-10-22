use std::os::fd::OwnedFd;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use crate::error::HostError;
use sha2::{Digest, Sha256};

use super::stage::OperationKey;
use super::{CAPTURE_DEADLINE, DiagnosticsRoot, check_capture_state};

#[path = "runner_logs/collect.rs"]
mod collect;
#[path = "runner_logs/persist.rs"]
mod persist;
#[path = "runner_logs/projection.rs"]
mod projection;

pub(in crate::launch) use collect::{
    collect_exec_output, create_exec_options, start_exec_options, validate_exec_exit,
    validate_exec_id,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RunnerLogAvailability {
    None,
    RunnerOnly,
    WorkerOnly,
    RunnerAndWorker,
}
pub(super) const MAX_ARCHIVE_BYTES: usize = 40 * 1024 * 1024;

pub(super) struct RunnerLogStageRequest {
    state: OwnedFd,
    parent: OwnedFd,
    scope: OwnedFd,
    key: OperationKey,
    generation: i64,
    container_id: String,
    projection: Vec<u8>,
    availability: RunnerLogAvailability,
    started: Instant,
}

impl RunnerLogStageRequest {
    pub(super) fn new(
        root: &DiagnosticsRoot,
        key: OperationKey,
        generation: i64,
        container_id: &str,
        archive: &[u8],
        started: Instant,
    ) -> Result<Self, HostError> {
        validate_request(root, &key, generation, container_id, archive, started)?;
        let sanitized = projection::sanitize_archive_with_availability(archive)?;
        Ok(Self {
            state: root.state.try_clone().map_err(|_| HostError::Path)?,
            parent: root.parent.try_clone().map_err(|_| HostError::Path)?,
            scope: root.scope.try_clone().map_err(|_| HostError::Path)?,
            key,
            generation,
            container_id: container_id.to_owned(),
            projection: sanitized.bytes,
            availability: sanitized.availability,
            started,
        })
    }

    pub(super) fn key(&self) -> &OperationKey {
        &self.key
    }

    pub(super) fn availability(&self) -> RunnerLogAvailability {
        self.availability
    }

    pub(super) fn projection_identity(&self) -> ([u8; 32], usize) {
        (
            Sha256::digest(&self.projection).into(),
            self.projection.len(),
        )
    }

    pub(super) fn write_request(self, cancelled: &AtomicBool) -> Result<(), HostError> {
        let deadline = self
            .started
            .checked_add(CAPTURE_DEADLINE)
            .ok_or(HostError::DockerTimeout)?;
        check_capture_state(self.started, deadline, cancelled)?;
        if !valid_container_id(&self.container_id)
            || self.key.intent_id <= 0
            || self.generation <= 0
        {
            return Err(HostError::Ownership);
        }
        persist::validate_private_directory(&self.state)?;
        persist::validate_private_directory(&self.parent)?;
        persist::validate_private_directory(&self.scope)?;
        let name = format!(
            "i{}-g{}-runner-logs.json",
            self.key.intent_id, self.generation
        );
        persist::write_projection(
            &self.scope,
            &name,
            &self.projection,
            self.started,
            deadline,
            cancelled,
        )
    }
}

fn validate_request(
    root: &DiagnosticsRoot,
    key: &OperationKey,
    generation: i64,
    container_id: &str,
    archive: &[u8],
    started: Instant,
) -> Result<(), HostError> {
    let deadline = started
        .checked_add(CAPTURE_DEADLINE)
        .ok_or(HostError::DockerTimeout)?;
    check_capture_state(started, deadline, &AtomicBool::new(false))?;
    if key.journal_identity != root.journal_identity
        || key.connection_id != root.connection_id
        || key.intent_id <= 0
        || generation <= 0
        || !valid_container_id(container_id)
        || archive.is_empty()
        || archive.len() > MAX_ARCHIVE_BYTES
    {
        return Err(HostError::Ownership);
    }
    Ok(())
}

fn valid_container_id(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
#[path = "runner_logs/tests.rs"]
mod tests;
