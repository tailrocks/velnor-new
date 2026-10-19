//! Local request listener sharing the daemon's journal and lineage guard.

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::config::{HostConfig, RepositoryConnection};
use crate::daemon_lock::DaemonLock;
use crate::error::HostError;
use crate::journal::Journal;

use super::wire::{DoctorProbeReply, Request, Response, SOCKET_NAME, Target};
use transport::{
    marker_exists, private_state_directory, read_request, remove_owned_socket, remove_stale_socket,
    trusted_peer, write_response,
};
#[path = "server/transport.rs"]
mod transport;
pub(super) use transport::{read_drain_token, valid_token};

const ACCEPT_PAUSE: Duration = Duration::from_millis(25);
const IO_TIMEOUT: Duration = Duration::from_secs(5);
const DOCTOR_REPORT_MAX_AGE: Duration = Duration::from_secs(30);

/// Live config bindings read by the private request listener.
#[derive(Debug, Clone)]
pub(crate) struct ControlState {
    bindings: Arc<RwLock<Vec<RepositoryConnection>>>,
    doctor_report: Arc<RwLock<Option<(String, serde_json::Value, Instant)>>>,
}

impl ControlState {
    #[cfg(test)]
    pub(super) fn empty() -> Self {
        Self {
            bindings: Arc::new(RwLock::new(Vec::new())),
            doctor_report: Arc::new(RwLock::new(None)),
        }
    }

    pub(crate) fn new(config: &HostConfig) -> Self {
        Self {
            bindings: Arc::new(RwLock::new(config.connections().to_vec())),
            doctor_report: Arc::new(RwLock::new(None)),
        }
    }

    pub(crate) fn replace_config(&self, config: &HostConfig) -> Result<(), HostError> {
        let mut bindings = self.bindings.write().map_err(|_| HostError::Journal)?;
        *bindings = config.connections().to_vec();
        Ok(())
    }

    pub(crate) fn contains_connection(
        &self,
        binding: &RepositoryConnection,
    ) -> Result<bool, HostError> {
        let bindings = self.bindings.read().map_err(|_| HostError::Journal)?;
        Ok(bindings
            .iter()
            .any(|candidate| candidate.connection_id == binding.connection_id))
    }

    pub(crate) fn save_doctor_report(
        &self,
        nonce: &str,
        report: serde_json::Value,
    ) -> Result<(), HostError> {
        *self.doctor_report.write().map_err(|_| HostError::Journal)? =
            Some((nonce.to_owned(), report, Instant::now()));
        Ok(())
    }

    fn doctor_report(&self, nonce: &str) -> Result<Option<serde_json::Value>, HostError> {
        let report = self.doctor_report.read().map_err(|_| HostError::Journal)?;
        Ok(report.as_ref().and_then(|(key, value, measured_at)| {
            (key == nonce
                && Instant::now()
                    .checked_duration_since(*measured_at)
                    .is_some_and(|age| age <= DOCTOR_REPORT_MAX_AGE))
            .then(|| value.clone())
        }))
    }
}

/// Local request listener whose requests run against the daemon-owned Journal.
pub(crate) struct ControlServer {
    path: PathBuf,
    socket_identity: (u64, u64),
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ControlServer {
    /// Bind the private socket and start its bounded listener thread.
    pub(crate) fn start(
        state: &Path,
        journal: Journal,
        config: &HostConfig,
        engine_id: String,
        docker_root_digest: String,
    ) -> Result<(Self, ControlState), HostError> {
        let state_path = private_state_directory(state)?;
        let path = state_path.join(SOCKET_NAME);
        remove_stale_socket(&path)?;
        let listener = UnixListener::bind(&path).map_err(|_| HostError::Lock)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .map_err(|_| HostError::Path)?;
        listener
            .set_nonblocking(true)
            .map_err(|_| HostError::Lock)?;
        let metadata = fs::symlink_metadata(&path).map_err(|_| HostError::Path)?;
        let identity = (metadata.dev(), metadata.ino());
        let state = ControlState::new(config);
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let thread_state = state.clone();
        let server_path = state_path.clone();
        let thread_engine_id = engine_id;
        let thread_root_digest = docker_root_digest;
        let handle = thread::Builder::new()
            .name("velnor-host-control".to_owned())
            .spawn(move || {
                serve(
                    &listener,
                    &journal,
                    &thread_state,
                    &server_path,
                    &thread_stop,
                    &thread_engine_id,
                    &thread_root_digest,
                );
            })
            .map_err(|_| HostError::Lock)?;
        Ok((
            Self {
                path,
                socket_identity: identity,
                stop,
                thread: Some(handle),
            },
            state,
        ))
    }
}

impl Drop for ControlServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        drop(UnixStream::connect(&self.path));
        if let Some(handle) = self.thread.take() {
            drop(handle.join());
        }
        remove_owned_socket(&self.path, self.socket_identity);
    }
}

fn serve(
    listener: &UnixListener,
    journal: &Journal,
    state: &ControlState,
    state_path: &Path,
    stop: &AtomicBool,
    engine_id: &str,
    docker_root_digest: &str,
) {
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return;
    };
    while !stop.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((mut stream, _)) => {
                if trusted_peer(&stream) {
                    if let Err(error) = stream.set_read_timeout(Some(IO_TIMEOUT)) {
                        eprintln!("host control read timeout setup failed: {error}");
                        continue;
                    }
                    if let Err(error) = stream.set_write_timeout(Some(IO_TIMEOUT)) {
                        eprintln!("host control write timeout setup failed: {error}");
                        continue;
                    }
                    let response = read_request(&mut stream).and_then(|request| {
                        runtime.block_on(handle_request(
                            journal,
                            state,
                            state_path,
                            request,
                            engine_id,
                            docker_root_digest,
                        ))
                    });
                    let response = response.unwrap_or(Response {
                        value: None,
                        doctor_probe: None,
                    });
                    if let Err(error) = write_response(&mut stream, response) {
                        eprintln!("host control response failed: {error}");
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(ACCEPT_PAUSE);
            }
            Err(_) => thread::sleep(ACCEPT_PAUSE),
        }
    }
}

async fn handle_request(
    journal: &Journal,
    state: &ControlState,
    state_path: &Path,
    request: Request,
    engine_id: &str,
    docker_root_digest: &str,
) -> Result<Response, HostError> {
    let _admission = DaemonLock::try_acquire(&state_path.join("admission.lock"))?;
    match request {
        Request::DrainStatus { target, token } => {
            let marker = target_marker(state_path, &target);
            if read_drain_token(&marker).as_deref() != Some(token.as_str()) {
                return Ok(Response {
                    value: Some(false),
                    doctor_probe: None,
                });
            }
            let drained = match target {
                Target::Host => journal.is_drained().await?,
                Target::Connection {
                    connection_id,
                    binding,
                } => {
                    if connection_id != binding.connection_id
                        || !configured_exactly(state, &binding)?
                    {
                        return Ok(Response {
                            value: Some(false),
                            doctor_probe: None,
                        });
                    }
                    journal
                        .connection_scope(&binding)
                        .await?
                        .is_drained()
                        .await?
                }
            };
            Ok(Response {
                value: Some(
                    drained && read_drain_token(&marker).as_deref() == Some(token.as_str()),
                ),
                doctor_probe: None,
            })
        }
        Request::Reactivate { binding, token } => {
            let marker = state_path
                .join("connection-drain")
                .join(binding.connection_id.as_str());
            let global_drain = state_path.join("drain");
            if marker_exists(&global_drain)?
                || read_drain_token(&marker).as_deref() != Some(token.as_str())
                || connection_id_present(state, &binding)?
            {
                return Ok(Response {
                    value: Some(false),
                    doctor_probe: None,
                });
            }
            let reactivated = journal.reactivate_if_drained(&binding).await?;
            Ok(Response {
                value: Some(reactivated),
                doctor_probe: None,
            })
        }
        Request::Detached { binding, token } => {
            let marker = state_path
                .join("connection-drain")
                .join(binding.connection_id.as_str());
            if read_drain_token(&marker).as_deref() != Some(token.as_str())
                || connection_id_present(state, &binding)?
            {
                return Ok(Response {
                    value: Some(false),
                    doctor_probe: None,
                });
            }
            let detached = journal.is_retired_and_drained(&binding).await?;
            Ok(Response {
                value: Some(
                    detached && read_drain_token(&marker).as_deref() == Some(token.as_str()),
                ),
                doctor_probe: None,
            })
        }
        Request::DoctorProbeStart { nonce } => {
            let reply = match journal
                .start_doctor_probe_request(&nonce, engine_id, docker_root_digest)
                .await?
            {
                crate::journal::DoctorProbeStartResult::Created(ticket)
                | crate::journal::DoctorProbeStartResult::Replayed(ticket) => DoctorProbeReply {
                    request_id: Some(ticket.request_id()),
                    state: "queued".to_owned(),
                    report: None,
                },
                crate::journal::DoctorProbeStartResult::Busy { active_request_id } => {
                    DoctorProbeReply {
                        request_id: Some(active_request_id),
                        state: "busy".to_owned(),
                        report: None,
                    }
                }
            };
            Ok(Response {
                value: None,
                doctor_probe: Some(reply),
            })
        }
        Request::DoctorProbeStatus { nonce } => {
            let reply = match journal.doctor_probe_request_by_nonce(&nonce).await? {
                Some(record) => DoctorProbeReply {
                    request_id: Some(record.request_id()),
                    state: record.state().as_str().to_owned(),
                    report: if record.state() == crate::journal::DoctorProbeState::Completed {
                        state.doctor_report(&nonce)?
                    } else {
                        None
                    },
                },
                None => DoctorProbeReply {
                    request_id: None,
                    state: "missing".to_owned(),
                    report: None,
                },
            };
            Ok(Response {
                value: None,
                doctor_probe: Some(reply),
            })
        }
    }
}

fn configured_exactly(
    state: &ControlState,
    binding: &RepositoryConnection,
) -> Result<bool, HostError> {
    let bindings = state.bindings.read().map_err(|_| HostError::Journal)?;
    Ok(bindings
        .iter()
        .any(|current| current.connection_id == binding.connection_id && current == binding))
}

fn connection_id_present(
    state: &ControlState,
    binding: &RepositoryConnection,
) -> Result<bool, HostError> {
    let bindings = state.bindings.read().map_err(|_| HostError::Journal)?;
    Ok(bindings
        .iter()
        .any(|current| current.connection_id == binding.connection_id))
}

fn target_marker(state: &Path, target: &Target) -> PathBuf {
    match target {
        Target::Host => state.join("drain"),
        Target::Connection { connection_id, .. } => {
            state.join("connection-drain").join(connection_id.as_str())
        }
    }
}

#[cfg(test)]
#[path = "server_tests.rs"]
mod tests;
