//! Exclusive offline requests after acquiring the daemon and engine locks.

use std::fs;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use crate::config::{HostConfig, RepositoryConnection};
use crate::daemon_lock::{DaemonLock, EngineLineageGuard, canonical_journal_path};
use crate::docker_client::connect_unix;
use crate::error::HostError;
use crate::journal::Journal;

use super::server::read_drain_token;
use super::wire::{Request, SOCKET_NAME, Target};

const MAX_CONFIG_BYTES: u64 = 1_048_576;

pub(super) fn call(state: &Path, request: &Request) -> Result<bool, HostError> {
    let _daemon = DaemonLock::try_acquire(&state.join("daemon.lock"))?;
    let _admission = DaemonLock::try_acquire(&state.join("admission.lock"))?;
    let result = with_journal(state, request)?;
    Ok(result)
}

fn with_journal(state: &Path, request: &Request) -> Result<bool, HostError> {
    let token = request_token(request);
    let marker = marker_path(state, request);
    if read_drain_token(&marker).as_deref() != Some(token) {
        return Ok(false);
    }
    let journal_path = state.join("launch.db");
    if !regular_file_exists(&journal_path)? {
        return Ok(false);
    }
    let (config, journal, _lineage) = open_existing_journal(state, &journal_path)?;
    let result = match request {
        Request::DrainStatus {
            target: Target::Host,
            ..
        } => run_status(&journal, None),
        Request::DrainStatus {
            target:
                Target::Connection {
                    connection_id,
                    binding,
                },
            ..
        } => {
            if connection_id != &binding.connection_id
                || config.connection(connection_id) != Some(binding.as_ref())
            {
                Ok(false)
            } else {
                run_status(&journal, Some(binding.as_ref()))
            }
        }
        Request::Reactivate { binding, .. } => {
            if marker_exists(&state.join("drain"))?
                || config.connection(&binding.connection_id).is_some()
            {
                Ok(false)
            } else {
                run_reactivate(&journal, binding)
            }
        }
        Request::Detached { binding, .. } => {
            if config.connection(&binding.connection_id).is_some() {
                Ok(false)
            } else {
                run_detached(&journal, binding)
            }
        }
        Request::DoctorProbeStart { .. } | Request::DoctorProbeStatus { .. } => {
            return Err(HostError::Lock);
        }
    }?;
    Ok(result && read_drain_token(&marker).as_deref() == Some(token))
}

fn run_status(
    journal: &Journal,
    binding: Option<&RepositoryConnection>,
) -> Result<bool, HostError> {
    let runtime = runtime()?;
    runtime.block_on(async {
        match binding {
            Some(binding) => journal.connection_scope(binding).await?.is_drained().await,
            None => journal.is_drained().await,
        }
    })
}

fn run_reactivate(journal: &Journal, binding: &RepositoryConnection) -> Result<bool, HostError> {
    let runtime = runtime()?;
    runtime.block_on(journal.reactivate_if_drained(binding))
}

fn run_detached(journal: &Journal, binding: &RepositoryConnection) -> Result<bool, HostError> {
    let runtime = runtime()?;
    runtime.block_on(journal.is_retired_and_drained(binding))
}

fn open_existing_journal(
    state: &Path,
    path: &Path,
) -> Result<(HostConfig, Journal, EngineLineageGuard), HostError> {
    let config = read_config(state)?;
    let docker = connect_unix(&config.docker().endpoint)?;
    let runtime = runtime()?;
    let engine_id = runtime
        .block_on(docker.info())
        .map_err(|_| HostError::Docker)?
        .id
        .ok_or(HostError::Docker)?;
    let lineage = EngineLineageGuard::acquire(&engine_id)?;
    let canonical = canonical_journal_path(path)?;
    let journal = runtime.block_on(Journal::open(&canonical))?;
    runtime.block_on(journal.bind_engine(&engine_id))?;
    let instance_id = runtime.block_on(journal.instance_id())?;
    let revision = runtime.block_on(journal.revision())?;
    lineage.verify_lineage(&canonical, &instance_id, revision)?;
    journal.attach_lineage_guard(lineage.clone())?;
    Ok((config, journal, lineage))
}

fn read_config(state: &Path) -> Result<HostConfig, HostError> {
    let path = state.join("host.toml");
    let mut options = fs::OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    let file = options.open(path).map_err(|_| HostError::Config)?;
    let metadata = file.metadata().map_err(|_| HostError::Config)?;
    if !metadata.is_file() || metadata.len() > MAX_CONFIG_BYTES {
        return Err(HostError::Config);
    }
    let mut bytes = Vec::new();
    file.take(MAX_CONFIG_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| HostError::Config)?;
    if u64::try_from(bytes.len()).map_or(true, |size| size > MAX_CONFIG_BYTES) {
        return Err(HostError::Config);
    }
    let text = String::from_utf8(bytes).map_err(|_| HostError::Config)?;
    HostConfig::parse(&text)
}

fn runtime() -> Result<tokio::runtime::Runtime, HostError> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| HostError::Journal)
}

fn request_token(request: &Request) -> &str {
    match request {
        Request::DrainStatus { token, .. }
        | Request::Reactivate { token, .. }
        | Request::Detached { token, .. } => token,
        Request::DoctorProbeStart { .. } | Request::DoctorProbeStatus { .. } => "",
    }
}

fn marker_path(state: &Path, request: &Request) -> std::path::PathBuf {
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
        Request::DoctorProbeStart { .. } | Request::DoctorProbeStatus { .. } => {
            state.join(SOCKET_NAME)
        }
    }
}

fn regular_file_exists(path: &Path) -> Result<bool, HostError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Ok(_) | Err(_) => Err(HostError::Path),
    }
}

fn marker_exists(path: &Path) -> Result<bool, HostError> {
    regular_file_exists(path)
}

#[cfg(test)]
mod tests {
    use super::call;
    use crate::controller_ipc::wire::{Request, Target};
    use std::fs;
    use std::path::PathBuf;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn missing_journal_does_not_prove_drain_or_create_lineage() -> Result<(), String> {
        let state = test_state();
        fs::create_dir_all(&state).map_err(|error| error.to_string())?;
        fs::write(state.join("drain"), TOKEN).map_err(|error| error.to_string())?;
        let drained = call(
            &state,
            &Request::DrainStatus {
                target: Target::Host,
                token: TOKEN.to_owned(),
            },
        )
        .map_err(|error| error.to_string())?;
        if drained || state.join("launch.db").exists() {
            return Err("missing journal was treated as drain proof".to_owned());
        }
        fs::remove_dir_all(state).map_err(|error| error.to_string())
    }

    fn test_state() -> PathBuf {
        std::env::temp_dir().join(format!(
            "velnor-offline-control-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ))
    }
}
