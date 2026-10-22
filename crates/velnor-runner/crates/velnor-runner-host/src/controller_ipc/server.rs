//! Local request listener sharing the daemon's journal and lineage guard.

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::config::{HostConfig, RepositoryConnection};
use crate::daemon_lock::DaemonLock;
use crate::error::HostError;
use crate::journal::Journal;

use super::wire::{Request, Response, SOCKET_NAME, Target};
use transport::{
    private_state_directory, read_request, remove_owned_socket, remove_stale_socket, trusted_peer,
    write_response,
};
#[path = "server/requests.rs"]
mod requests;
mod state;
pub(crate) use state::{ControlState, ControllerReadinessProof, SessionPollReceipt};
#[path = "server/transport.rs"]
mod transport;
pub(super) use transport::{read_drain_token, valid_token};

const ACCEPT_PAUSE: Duration = Duration::from_millis(25);
const IO_TIMEOUT: Duration = Duration::from_secs(5);

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
                        readiness: None,
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
    requests::handle(
        journal,
        state,
        state_path,
        request,
        engine_id,
        docker_root_digest,
    )
    .await
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
