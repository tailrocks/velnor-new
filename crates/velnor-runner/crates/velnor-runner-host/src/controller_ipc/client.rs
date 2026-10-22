//! Synchronous CLI requests to the lineage-owning daemon.

use std::fs;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use super::server::{read_drain_token, valid_token};
use super::wire::{Request, Response, SOCKET_NAME, Target};
use crate::config::{ConnectionId, HostConfig, RepositoryConnection};
use crate::error::HostError;

mod doctor_result;
mod readiness;
mod transport;

use transport::call_socket_response_with_timeout;

pub use readiness::controller_readiness_status;

const RETRY: Duration = Duration::from_millis(100);
const CALL_TIMEOUT: Duration = Duration::from_secs(5);
const DOCTOR_TIMEOUT_MAX: Duration = Duration::from_secs(300);
const MAX_CONFIG_BYTES: u64 = 1_048_576;
const HEX: &[u8; 16] = b"0123456789abcdef";

/// Run one bounded Doctor probe through the active controller.
///
/// # Errors
///
/// Returns an error if the request is invalid or the controller, journal, or probe operation fails.
pub fn run_doctor_probe(state: &Path, timeout: Duration) -> Result<serde_json::Value, HostError> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| HostError::Identity)?;
    let nonce = bytes
        .iter()
        .flat_map(|byte| [HEX[usize::from(byte >> 4)], HEX[usize::from(byte & 0x0f)]])
        .map(char::from)
        .collect::<String>();
    let start_response = doctor_call(
        state,
        &Request::DoctorProbeStart {
            nonce: nonce.clone(),
        },
    )?;
    let start = start_response.doctor_probe.ok_or(HostError::Journal)?;
    if start.state == "busy" {
        return Err(HostError::Journal);
    }
    let deadline = Instant::now()
        .checked_add(timeout.min(DOCTOR_TIMEOUT_MAX))
        .ok_or(HostError::Identity)?;
    loop {
        let response = doctor_call(
            state,
            &Request::DoctorProbeStatus {
                nonce: nonce.clone(),
            },
        )?;
        let readiness = response.readiness;
        let reply = response.doctor_probe.ok_or(HostError::Journal)?;
        match reply.state.as_str() {
            "completed" => {
                if let Some(result) = doctor_result::completed_result(reply, readiness) {
                    return Ok(result);
                }
            }
            "queued" | "running" => {}
            _ => return Err(HostError::Identity),
        }
        if Instant::now() >= deadline {
            return Err(HostError::Docker);
        }
        thread::sleep(RETRY.min(deadline.saturating_duration_since(Instant::now())));
    }
}

fn doctor_call(state: &Path, request: &Request) -> Result<Response, HostError> {
    if !valid_request(request) {
        return Err(HostError::Frame);
    }
    let path = state.join(SOCKET_NAME);
    call_socket_response(&path, request)
}

/// Wait until the host journal is empty while a matching global drain token stays active.
///
/// # Errors
///
/// Returns an error when the local control channel, journal lineage, or request is invalid.
pub fn wait_for_drain(state: &Path, timeout: Duration) -> Result<bool, HostError> {
    let marker = state.join("drain");
    let Some(token) = read_drain_token(&marker) else {
        return Ok(false);
    };
    wait_until(state, RequestTarget::Host, &token, timeout)
}

/// Wait until one configured connection is empty while its drain token stays active.
///
/// # Errors
///
/// Returns an error when the config, local control channel, journal lineage, or request is invalid.
pub fn wait_for_connection_drain(
    state: &Path,
    connection_id: &ConnectionId,
    timeout: Duration,
) -> Result<bool, HostError> {
    let marker = state.join("connection-drain").join(connection_id.as_str());
    let Some(token) = read_drain_token(&marker) else {
        return Ok(false);
    };
    let Some(binding) = read_connection(state, connection_id)? else {
        return Ok(false);
    };
    wait_until(state, RequestTarget::Connection(binding), &token, timeout)
}

/// Wait until an exact retired connection stays detached.
///
/// # Errors
///
/// Returns an error when the local control channel, journal lineage, or request is invalid.
pub fn wait_for_connection_detached(
    state: &Path,
    binding: &RepositoryConnection,
    timeout: Duration,
) -> Result<bool, HostError> {
    let marker = state
        .join("connection-drain")
        .join(binding.connection_id.as_str());
    let Some(token) = read_drain_token(&marker) else {
        return Ok(false);
    };
    wait_until(
        state,
        RequestTarget::Detached(binding.clone()),
        &token,
        timeout,
    )
}

/// Ask the daemon to reactivate only an exact, drained connection tombstone.
///
/// # Errors
///
/// Returns an error when the local control channel, journal lineage, or request is invalid.
pub fn reactivate_connection_if_drained(
    state: &Path,
    binding: &RepositoryConnection,
) -> Result<bool, HostError> {
    let marker = state
        .join("connection-drain")
        .join(binding.connection_id.as_str());
    let Some(token) = read_drain_token(&marker) else {
        return Ok(false);
    };
    call(
        state,
        &Request::Reactivate {
            binding: binding.clone(),
            token,
        },
    )
}

enum RequestTarget {
    Host,
    Connection(RepositoryConnection),
    Detached(RepositoryConnection),
}

fn wait_until(
    state: &Path,
    target: RequestTarget,
    token: &str,
    timeout: Duration,
) -> Result<bool, HostError> {
    let deadline = Instant::now() + timeout;
    let request = match target {
        RequestTarget::Host => Request::DrainStatus {
            target: Target::Host,
            token: token.to_owned(),
        },
        RequestTarget::Connection(binding) => Request::DrainStatus {
            target: Target::Connection {
                connection_id: binding.connection_id.clone(),
                binding: Box::new(binding),
            },
            token: token.to_owned(),
        },
        RequestTarget::Detached(binding) => Request::Detached {
            binding,
            token: token.to_owned(),
        },
    };
    loop {
        if read_drain_token(&marker_for(state, &request)).as_deref() != Some(token) {
            return Ok(false);
        }
        if call(state, &request)? {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        thread::sleep(RETRY.min(deadline.saturating_duration_since(Instant::now())));
    }
}

fn marker_for(state: &Path, request: &Request) -> std::path::PathBuf {
    match request {
        Request::DrainStatus {
            target: Target::Host,
            ..
        } => state.join("drain"),
        Request::DrainStatus {
            target: Target::Connection { connection_id, .. },
            ..
        } => state.join("connection-drain").join(connection_id.as_str()),
        Request::Reactivate { binding, .. } | Request::Detached { binding, .. } => state
            .join("connection-drain")
            .join(binding.connection_id.as_str()),
        Request::DoctorProbeStart { .. }
        | Request::DoctorProbeStatus { .. }
        | Request::ControllerReadiness { .. } => state.join(SOCKET_NAME),
    }
}

fn call(state: &Path, request: &Request) -> Result<bool, HostError> {
    if !valid_request(request) {
        return Err(HostError::Frame);
    }
    if matches!(
        request,
        Request::DoctorProbeStart { .. }
            | Request::DoctorProbeStatus { .. }
            | Request::ControllerReadiness { .. }
    ) {
        return Err(HostError::Lock);
    }
    let path = state.join(SOCKET_NAME);
    match call_socket(&path, request) {
        Ok(value) => Ok(value),
        Err(HostError::Lock) => super::offline::call(state, request),
        Err(error) => Err(error),
    }
}

fn call_socket(path: &Path, request: &Request) -> Result<bool, HostError> {
    call_socket_response(path, request)?
        .value
        .ok_or(HostError::Journal)
}

fn call_socket_response(path: &Path, request: &Request) -> Result<Response, HostError> {
    call_socket_response_with_timeout(path, request, CALL_TIMEOUT)
}

fn read_connection(
    state: &Path,
    id: &ConnectionId,
) -> Result<Option<RepositoryConnection>, HostError> {
    let path = state.join("host.toml");
    let Some(text) = read_bounded_file(&path, MAX_CONFIG_BYTES)? else {
        return Ok(None);
    };
    let config = HostConfig::parse(&text)?;
    Ok(config.connection(id).cloned())
}

fn read_bounded_file(path: &Path, max_bytes: u64) -> Result<Option<String>, HostError> {
    let mut options = fs::OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(HostError::Path),
    };
    let metadata = file.metadata().map_err(|_| HostError::Path)?;
    if !metadata.is_file() || metadata.len() > max_bytes {
        return Err(HostError::Path);
    }
    let mut bytes = Vec::new();
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| HostError::Path)?;
    if u64::try_from(bytes.len()).map_or(true, |size| size > max_bytes) {
        return Err(HostError::Path);
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| HostError::Config)
}

fn valid_request(request: &Request) -> bool {
    match request {
        Request::DrainStatus { target, token } => {
            valid_token(token)
                && match target {
                    Target::Host => true,
                    Target::Connection {
                        connection_id,
                        binding,
                    } => connection_id == &binding.connection_id,
                }
        }
        Request::Reactivate { binding, token } | Request::Detached { binding, token } => {
            let _ = binding;
            valid_token(token)
        }
        Request::DoctorProbeStart { nonce } | Request::DoctorProbeStatus { nonce } => {
            nonce.len() == 32
                && nonce
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        }
        Request::ControllerReadiness {
            config_sha256,
            engine_id,
            docker_root_digest,
        } => {
            valid_digest(config_sha256)
                && !engine_id.is_empty()
                && engine_id.len() <= 128
                && valid_digest(docker_root_digest)
        }
    }
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
