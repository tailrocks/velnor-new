// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use super::super::server::read_drain_token;
use super::super::wire::{Request, SOCKET_NAME, ScaleSetDeleteStatus};
use super::{HEX, RETRY, call_socket_response, valid_request};
use crate::config::RepositoryConnection;
use crate::error::HostError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreatedSetDeletion {
    Deleted,

    NotOwned,

    Refused(u16),

    Uncertain,

    TimedOut,

    ControllerOffline,
}

pub fn delete_created_scale_set(
    state: &Path,
    binding: &RepositoryConnection,
    timeout: Duration,
) -> Result<CreatedSetDeletion, HostError> {
    let marker = state
        .join("connection-drain")
        .join(binding.connection_id.as_str());
    let Some(token) = read_drain_token(&marker) else {
        return Ok(CreatedSetDeletion::Uncertain);
    };
    let nonce = random_nonce()?;
    let start = Request::ScaleSetDeleteStart {
        binding: binding.clone(),
        token: token.clone(),
        nonce: nonce.clone(),
    };
    let status = Request::ScaleSetDeleteStatus {
        connection_id: binding.connection_id.clone(),
        nonce,
    };
    if !valid_request(&start) || !valid_request(&status) {
        return Err(HostError::Frame);
    }
    let deadline = Instant::now() + timeout;
    let socket = state.join(SOCKET_NAME);
    let mut reply = match call_socket_response(&socket, &start) {
        Ok(response) => response,
        Err(HostError::Lock) => {
            return super::super::offline::scale_set_delete(state, binding, &token);
        }
        Err(error) => return Err(error),
    };
    loop {
        let current = reply.scale_set_delete.ok_or(HostError::Journal)?.status;
        match current {
            ScaleSetDeleteStatus::Pending => {}
            ScaleSetDeleteStatus::Deleted => return Ok(CreatedSetDeletion::Deleted),
            ScaleSetDeleteStatus::NotOwned => return Ok(CreatedSetDeletion::NotOwned),
            ScaleSetDeleteStatus::Refused { status } => {
                return Ok(CreatedSetDeletion::Refused(status));
            }
            ScaleSetDeleteStatus::Uncertain | ScaleSetDeleteStatus::Stale => {
                return Ok(CreatedSetDeletion::Uncertain);
            }
        }
        if read_drain_token(&marker).as_deref() != Some(token.as_str()) {
            return Ok(CreatedSetDeletion::Uncertain);
        }
        if Instant::now() >= deadline {
            return Ok(CreatedSetDeletion::TimedOut);
        }
        thread::sleep(RETRY.min(deadline.saturating_duration_since(Instant::now())));
        reply = call_socket_response(&socket, &status)?;
    }
}

fn random_nonce() -> Result<String, HostError> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| HostError::Identity)?;
    Ok(bytes
        .iter()
        .flat_map(|byte| [HEX[usize::from(byte >> 4)], HEX[usize::from(byte & 0x0f)]])
        .map(char::from)
        .collect())
}
