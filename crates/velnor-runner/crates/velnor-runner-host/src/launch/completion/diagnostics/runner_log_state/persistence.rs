use std::os::fd::OwnedFd;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use sha2::{Digest, Sha256};

use crate::error::HostError;

use super::super::stage::OperationKey;
use super::super::{CAPTURE_DEADLINE, DiagnosticsRoot, check_capture_state};
use super::{
    CaptureBinding, CapturePhase, CaptureState, PROJECTION_BYTES, STATE_BYTES, lower_hex, storage,
    valid_digest, valid_id,
};

pub(in crate::launch) struct CaptureBindingInput<'a> {
    pub(in crate::launch) intent_id: i64,
    pub(in crate::launch) reservation_id: i64,
    pub(in crate::launch) operation_id: i64,
    pub(in crate::launch) topology_version: i64,
    pub(in crate::launch) engine_id: &'a str,
    pub(in crate::launch) docker_root_digest: &'a str,
    pub(in crate::launch) runner_container_id: &'a str,
    pub(in crate::launch) dind_container_id: &'a str,
    pub(in crate::launch) runner_home_volume: &'a str,
}

pub(in crate::launch) struct CaptureStateRequest {
    scope: OwnedFd,
    key: OperationKey,
    name: String,
    bytes: Vec<u8>,
    started: Instant,
}

impl CaptureStateRequest {
    pub(in crate::launch) fn new(
        root: &DiagnosticsRoot,
        binding: &CaptureBinding,
        state: &CaptureState,
        started: Instant,
    ) -> Result<Self, HostError> {
        state.validate(binding)?;
        if binding.journal_identity != root.journal_identity
            || binding.connection_id != root.connection_id
        {
            return Err(HostError::Ownership);
        }
        let bytes = serde_json::to_vec(state).map_err(|_| HostError::Journal)?;
        if bytes.is_empty() || bytes.len() > STATE_BYTES {
            return Err(HostError::Ownership);
        }
        Ok(Self {
            scope: root.scope.try_clone().map_err(|_| HostError::Path)?,
            key: OperationKey {
                journal_identity: binding.journal_identity.clone(),
                connection_id: binding.connection_id.clone(),
                intent_id: binding.intent_id,
            },
            name: state_name(binding.intent_id, binding.reservation_id)?,
            bytes,
            started,
        })
    }

    pub(in crate::launch) fn key(&self) -> &OperationKey {
        &self.key
    }

    pub(in crate::launch) fn write_request(self, cancelled: &AtomicBool) -> Result<(), HostError> {
        let deadline = self
            .started
            .checked_add(CAPTURE_DEADLINE)
            .ok_or(HostError::DockerTimeout)?;
        check_capture_state(self.started, deadline, cancelled)?;
        storage::write_state(
            &self.scope,
            &self.name,
            &self.bytes,
            self.started,
            deadline,
            cancelled,
        )
    }
}

pub(in crate::launch) fn binding(
    root: &DiagnosticsRoot,
    input: &CaptureBindingInput<'_>,
) -> Result<CaptureBinding, HostError> {
    if input.intent_id <= 0
        || input.reservation_id <= 0
        || input.operation_id <= 0
        || input.topology_version <= 0
        || input.engine_id.is_empty()
        || !valid_digest(input.docker_root_digest)
        || !valid_id(input.runner_container_id)
        || !valid_id(input.dind_container_id)
        || input.runner_container_id == input.dind_container_id
        || !safe_component(input.runner_home_volume)
    {
        return Err(HostError::Ownership);
    }
    Ok(CaptureBinding {
        journal_identity: root.journal_identity.clone(),
        connection_id: root.connection_id.clone(),
        intent_id: input.intent_id,
        reservation_id: input.reservation_id,
        operation_id: input.operation_id,
        topology_version: input.topology_version,
        engine_id: input.engine_id.to_owned(),
        docker_root_digest: input.docker_root_digest.to_owned(),
        runner_container_id: input.runner_container_id.to_owned(),
        dind_container_id: input.dind_container_id.to_owned(),
        runner_home_volume: input.runner_home_volume.to_owned(),
    })
}

pub(in crate::launch) fn read_state(
    root: &DiagnosticsRoot,
    binding: &CaptureBinding,
) -> Result<Option<CaptureState>, HostError> {
    let name = state_name(binding.intent_id, binding.reservation_id)?;
    let Some(bytes) = storage::read_private(&root.scope, &name, STATE_BYTES)? else {
        return Ok(None);
    };
    let state: CaptureState = serde_json::from_slice(&bytes).map_err(|_| HostError::Ownership)?;
    state.validate(binding)?;
    Ok(Some(state))
}

pub(in crate::launch) fn read_projection(
    root: &DiagnosticsRoot,
    binding: &CaptureBinding,
    state: &CaptureState,
) -> Result<Vec<u8>, HostError> {
    state.validate(binding)?;
    let name = projection_name(binding.intent_id, state.attempt)?;
    let bytes =
        storage::read_private(&root.scope, &name, PROJECTION_BYTES)?.ok_or(HostError::Path)?;
    if !matches_projection_identity(&bytes, state) {
        return Err(HostError::Ownership);
    }
    Ok(bytes)
}

pub(in crate::launch) fn recover_terminal_projection(
    root: &DiagnosticsRoot,
    binding: &CaptureBinding,
    state: &CaptureState,
) -> Result<Option<(bool, String, u64)>, HostError> {
    state.validate(binding)?;
    if state.phase != CapturePhase::ProjectionPending {
        return Err(HostError::Ownership);
    }
    let name = projection_name(binding.intent_id, state.attempt)?;
    let Some(bytes) = storage::read_private(&root.scope, &name, PROJECTION_BYTES)? else {
        return Ok(None);
    };
    let (digest, size) = projection_identity(&bytes)?;
    if state.projection_sha256.as_deref() != Some(digest.as_str())
        || state.projection_bytes != Some(size)
        || state.has_trace.is_none()
    {
        return Err(HostError::Ownership);
    }
    Ok(Some((state.has_trace == Some(true), digest, size)))
}

fn matches_projection_identity(bytes: &[u8], state: &CaptureState) -> bool {
    Some(bytes.len() as u64) == state.projection_bytes
        && Some(lower_hex(&Sha256::digest(bytes))) == state.projection_sha256
        && state.has_trace.is_some()
}

pub(in crate::launch) fn projection_identity(bytes: &[u8]) -> Result<(String, u64), HostError> {
    if bytes.is_empty() || bytes.len() > PROJECTION_BYTES {
        return Err(HostError::Ownership);
    }
    Ok((lower_hex(&Sha256::digest(bytes)), bytes.len() as u64))
}

fn state_name(intent_id: i64, reservation_id: i64) -> Result<String, HostError> {
    if intent_id <= 0 || reservation_id <= 0 {
        return Err(HostError::Ownership);
    }
    Ok(format!(
        "i{intent_id}-r{reservation_id}-runner-log-state.json"
    ))
}

fn projection_name(intent_id: i64, attempt: i64) -> Result<String, HostError> {
    if intent_id <= 0 || attempt <= 0 {
        return Err(HostError::Ownership);
    }
    Ok(format!("i{intent_id}-g{attempt}-runner-logs.json"))
}

fn safe_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
        && value != "."
        && value != ".."
}
