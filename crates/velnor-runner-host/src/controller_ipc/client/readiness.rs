// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::time::Duration;

use crate::error::HostError;
use crate::readiness::Readiness;

use super::super::wire::Request;

pub fn controller_readiness_status(
    state: &Path,
    timeout: Duration,
    config_sha256: &str,
    engine_id: &str,
    docker_root_digest: &str,
) -> Result<Readiness, HostError> {
    if timeout.is_zero() {
        return Err(HostError::Frame);
    }
    let request = Request::ControllerReadiness {
        config_sha256: config_sha256.to_owned(),
        engine_id: engine_id.to_owned(),
        docker_root_digest: docker_root_digest.to_owned(),
    };
    if !super::valid_request(&request) {
        return Err(HostError::Frame);
    }
    let response = super::call_socket_response_with_timeout(
        &state.join(super::SOCKET_NAME),
        &request,
        timeout.min(super::CALL_TIMEOUT),
    )?;
    response.readiness.ok_or(HostError::Journal)
}
