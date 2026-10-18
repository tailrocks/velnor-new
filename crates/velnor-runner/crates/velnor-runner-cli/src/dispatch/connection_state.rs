use std::path::{Path, PathBuf};

use std::io::Read;

use velnor_runner_host::{
    ConnectionId, DaemonLock, HostConfig, HostError, KeychainReference, RepositoryConnection,
    read_secret,
};

use super::connect::ConnectError;
use super::marker::{MarkerToken, StateWriteError, atomic_write};

pub(super) fn read_host(state: &Path) -> Result<Option<HostConfig>, ConnectError> {
    const MAX_BYTES: u64 = 1_048_576;
    let path = state.join("host.toml");
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata)
            if metadata.is_file()
                && !metadata.file_type().is_symlink()
                && metadata.len() <= MAX_BYTES =>
        {
            metadata
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Ok(_) | Err(_) => return Err(ConnectError::Config),
    };
    let raw = std::fs::read_to_string(&path).map_err(|_| ConnectError::Config)?;
    let Ok(length) = u64::try_from(raw.len()) else {
        return Err(ConnectError::Config);
    };
    if length > MAX_BYTES || length > metadata.len() {
        return Err(ConnectError::Config);
    }
    HostConfig::parse(&raw)
        .map(Some)
        .map_err(|_| ConnectError::Config)
}

pub(super) fn store_token<R, F>(
    input: &mut R,
    credential_ref: &KeychainReference,
    store: F,
) -> Result<(), ConnectError>
where
    R: Read,
    F: FnOnce(&str, &str, &[u8]) -> Result<(), HostError>,
{
    let secret = read_secret(input).map_err(ConnectError::Secret)?;
    store(credential_ref.service(), credential_ref.account(), &secret).map_err(ConnectError::Secret)
}

pub(super) fn persist_host(state: &Path, text: &str) -> Result<(), ConnectError> {
    const MAX_BYTES: usize = 1_048_576;
    if text.len() > MAX_BYTES {
        return Err(ConnectError::Write);
    }
    atomic_write(&state.join("host.toml"), text.as_bytes()).map_err(connect_write_error)
}

pub(super) fn connect_write_error(error: StateWriteError) -> ConnectError {
    match error {
        StateWriteError::Failed => ConnectError::Write,
        StateWriteError::Uncertain => ConnectError::WriteUncertain,
    }
}

pub(super) fn config_lock(state: &Path) -> Result<DaemonLock, HostError> {
    DaemonLock::try_acquire(&state.join("config.lock"))
}

pub(super) fn admission_lock(state: &Path) -> Result<DaemonLock, HostError> {
    DaemonLock::try_acquire(&state.join("admission.lock"))
}

pub(super) fn connection_lifecycle_lock(
    state: &Path,
    connection_id: &ConnectionId,
) -> Result<DaemonLock, HostError> {
    DaemonLock::try_acquire(
        &state
            .join("connection-lifecycle")
            .join(format!("{}.lock", connection_id.as_str())),
    )
}

pub(super) fn connection_drain_marker(state: &Path, connection_id: &ConnectionId) -> PathBuf {
    state.join("connection-drain").join(connection_id.as_str())
}

pub(super) fn connection_binding_snapshot(state: &Path, connection_id: &ConnectionId) -> PathBuf {
    state
        .join("connection-drain")
        .join(format!("{}~binding", connection_id.as_str()))
}

pub(super) fn connection_reactivation_proof(state: &Path, connection_id: &ConnectionId) -> PathBuf {
    state
        .join("connection-drain")
        .join(format!("{}~reactivation", connection_id.as_str()))
}

pub(super) fn connection_drain_proof(state: &Path, connection_id: &ConnectionId) -> PathBuf {
    state
        .join("connection-drain")
        .join(format!("{}~drained", connection_id.as_str()))
}

pub(super) fn write_reactivation_proof(
    state: &Path,
    connection: &RepositoryConnection,
    token: &MarkerToken,
) -> Result<(), StateWriteError> {
    write_binding_proof(
        &connection_reactivation_proof(state, &connection.connection_id),
        connection,
        token,
    )
}

pub(super) fn write_drain_proof(
    state: &Path,
    connection: &RepositoryConnection,
    token: &MarkerToken,
) -> Result<(), StateWriteError> {
    write_binding_proof(
        &connection_drain_proof(state, &connection.connection_id),
        connection,
        token,
    )
}

fn write_binding_proof(
    path: &Path,
    connection: &RepositoryConnection,
    token: &MarkerToken,
) -> Result<(), StateWriteError> {
    let identity = connection
        .canonical_identity_bytes()
        .map_err(|_| StateWriteError::Failed)?;
    let mut bytes = Vec::with_capacity(token.0.len() + 1 + identity.len());
    bytes.extend_from_slice(token.0.as_bytes());
    bytes.push(b'\n');
    bytes.extend_from_slice(&identity);
    atomic_write(path, &bytes)
}

pub(super) fn has_reactivation_proof(
    state: &Path,
    connection_id: &ConnectionId,
    connection: &RepositoryConnection,
    expected_token: &MarkerToken,
) -> Result<bool, ConnectError> {
    has_binding_proof(
        &connection_reactivation_proof(state, connection_id),
        connection_id,
        connection,
        expected_token,
    )
}

pub(super) fn has_drain_proof(
    state: &Path,
    connection_id: &ConnectionId,
    connection: &RepositoryConnection,
    expected_token: &MarkerToken,
) -> Result<bool, ConnectError> {
    has_binding_proof(
        &connection_drain_proof(state, connection_id),
        connection_id,
        connection,
        expected_token,
    )
}

fn has_binding_proof(
    path: &Path,
    connection_id: &ConnectionId,
    connection: &RepositoryConnection,
    expected_token: &MarkerToken,
) -> Result<bool, ConnectError> {
    const MAX_BYTES: u64 = 1_048_609;
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata)
            if metadata.is_file()
                && !metadata.file_type().is_symlink()
                && metadata.len() <= MAX_BYTES =>
        {
            metadata
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Ok(_) | Err(_) => return Err(ConnectError::Rejected),
    };
    let bytes = std::fs::read(path).map_err(|_| ConnectError::Rejected)?;
    let Ok(length) = u64::try_from(bytes.len()) else {
        return Err(ConnectError::Rejected);
    };
    if length > MAX_BYTES || length > metadata.len() || bytes.len() <= 33 || bytes[32] != b'\n' {
        return Err(ConnectError::Rejected);
    }
    let Some(token) = MarkerToken::parse(&bytes[..32]) else {
        return Err(ConnectError::Rejected);
    };
    let proof_connection: RepositoryConnection =
        serde_json::from_slice(&bytes[33..]).map_err(|_| ConnectError::Rejected)?;
    if &proof_connection.connection_id != connection_id
        || proof_connection
            .canonical_identity_bytes()
            .map_err(|_| ConnectError::Rejected)?
            != bytes[33..]
    {
        return Err(ConnectError::Rejected);
    }
    if &token != expected_token {
        return Ok(false);
    }
    same_connection_identity(&proof_connection, connection)
}

pub(super) fn write_connection_snapshot(
    state: &Path,
    connection_id: &ConnectionId,
    connection: &RepositoryConnection,
) -> Result<(), StateWriteError> {
    if &connection.connection_id != connection_id {
        return Err(StateWriteError::Failed);
    }
    let identity = connection
        .canonical_identity_bytes()
        .map_err(|_| StateWriteError::Failed)?;
    atomic_write(
        &connection_binding_snapshot(state, connection_id),
        &identity,
    )
}

pub(super) fn read_connection_snapshot(
    state: &Path,
    connection_id: &ConnectionId,
) -> Result<Option<RepositoryConnection>, ConnectError> {
    const MAX_BYTES: u64 = 1_048_576;
    let path = connection_binding_snapshot(state, connection_id);
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata)
            if metadata.is_file()
                && !metadata.file_type().is_symlink()
                && metadata.len() <= MAX_BYTES =>
        {
            metadata
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Ok(_) | Err(_) => return Err(ConnectError::Rejected),
    };
    let bytes = std::fs::read(&path).map_err(|_| ConnectError::Rejected)?;
    let Ok(length) = u64::try_from(bytes.len()) else {
        return Err(ConnectError::Rejected);
    };
    if length > MAX_BYTES || length > metadata.len() {
        return Err(ConnectError::Rejected);
    }
    let connection: RepositoryConnection =
        serde_json::from_slice(&bytes).map_err(|_| ConnectError::Rejected)?;
    if &connection.connection_id != connection_id
        || connection
            .canonical_identity_bytes()
            .map_err(|_| ConnectError::Rejected)?
            != bytes
    {
        return Err(ConnectError::Rejected);
    }
    Ok(Some(connection))
}

pub(super) fn same_connection_identity(
    left: &RepositoryConnection,
    right: &RepositoryConnection,
) -> Result<bool, ConnectError> {
    let left = left
        .canonical_identity_bytes()
        .map_err(|_| ConnectError::Config)?;
    let right = right
        .canonical_identity_bytes()
        .map_err(|_| ConnectError::Config)?;
    Ok(left == right)
}
