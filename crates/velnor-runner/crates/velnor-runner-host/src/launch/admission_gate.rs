//! Fence new work against drain and config removal.

use std::fs;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use crate::config::{HostConfig, RepositoryConnection};
use crate::daemon_lock::DaemonLock;
use crate::error::HostError;
use crate::scale_set::EnsureError;

const MAX_CONFIG_BYTES: u64 = 1_048_576;

/// Lock admission during a check of the exact active binding and drain markers.
pub(crate) fn lock_connection(
    state: &Path,
    binding: &RepositoryConnection,
) -> Result<DaemonLock, EnsureError> {
    let lock = DaemonLock::try_acquire(&state.join("admission.lock"))
        .map_err(|_| EnsureError::AdmissionClosed)?;
    if marker_present(&state.join("drain"))?
        || marker_present(
            &state
                .join("connection-drain")
                .join(binding.connection_id.as_str()),
        )?
        || !binding_is_active(state, binding)?
    {
        return Err(EnsureError::AdmissionClosed);
    }
    Ok(lock)
}

fn binding_is_active(state: &Path, binding: &RepositoryConnection) -> Result<bool, EnsureError> {
    let path = state.join("host.toml");
    let mut options = fs::OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    let file = options
        .open(path)
        .map_err(|_| EnsureError::AdmissionClosed)?;
    let metadata = file.metadata().map_err(|_| EnsureError::AdmissionClosed)?;
    if !metadata.is_file() || metadata.len() > MAX_CONFIG_BYTES {
        return Err(EnsureError::AdmissionClosed);
    }
    let mut bytes = Vec::new();
    file.take(MAX_CONFIG_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| EnsureError::AdmissionClosed)?;
    if u64::try_from(bytes.len()).map_or(true, |length| length > MAX_CONFIG_BYTES) {
        return Err(EnsureError::AdmissionClosed);
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| EnsureError::AdmissionClosed)?;
    let config = HostConfig::parse(text).map_err(map_host)?;
    Ok(config.connection(&binding.connection_id) == Some(binding))
}

fn marker_present(path: &Path) -> Result<bool, EnsureError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Ok(_) | Err(_) => Err(EnsureError::AdmissionClosed),
    }
}

fn map_host(error: HostError) -> EnsureError {
    match error {
        HostError::Endpoint => EnsureError::Endpoint,
        _ => EnsureError::AdmissionClosed,
    }
}
