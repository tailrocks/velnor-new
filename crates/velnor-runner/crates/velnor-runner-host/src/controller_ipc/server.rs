//! Local request listener sharing the daemon's journal and lineage guard.

use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::config::{HostConfig, RepositoryConnection};
use crate::daemon_lock::DaemonLock;
use crate::error::HostError;
use crate::ipc::{MAX_FRAME, encode_frame};
use crate::journal::Journal;

use super::wire::{MAX_REQUEST_BYTES, Request, Response, SOCKET_NAME, Target};

const ACCEPT_PAUSE: Duration = Duration::from_millis(25);
const IO_TIMEOUT: Duration = Duration::from_secs(5);

/// Live config bindings read by the private request listener.
#[derive(Debug, Clone)]
pub(crate) struct ControlState(Arc<RwLock<Vec<RepositoryConnection>>>);

impl ControlState {
    pub(crate) fn new(config: &HostConfig) -> Self {
        Self(Arc::new(RwLock::new(config.connections().to_vec())))
    }

    pub(crate) fn replace_config(&self, config: &HostConfig) -> Result<(), HostError> {
        let mut bindings = self.0.write().map_err(|_| HostError::Journal)?;
        *bindings = config.connections().to_vec();
        Ok(())
    }

    pub(crate) fn contains_connection(
        &self,
        binding: &RepositoryConnection,
    ) -> Result<bool, HostError> {
        let bindings = self.0.read().map_err(|_| HostError::Journal)?;
        Ok(bindings
            .iter()
            .any(|candidate| candidate.connection_id == binding.connection_id))
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
        let handle = thread::Builder::new()
            .name("velnor-host-control".to_owned())
            .spawn(move || {
                serve(
                    &listener,
                    &journal,
                    &thread_state,
                    &server_path,
                    &thread_stop,
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
                        runtime.block_on(handle_request(journal, state, state_path, request))
                    });
                    let response = response.unwrap_or(Response { value: None });
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
) -> Result<Response, HostError> {
    let _admission = DaemonLock::try_acquire(&state_path.join("admission.lock"))?;
    match request {
        Request::DrainStatus { target, token } => {
            let marker = target_marker(state_path, &target);
            if read_drain_token(&marker).as_deref() != Some(token.as_str()) {
                return Ok(Response { value: Some(false) });
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
                        return Ok(Response { value: Some(false) });
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
                return Ok(Response { value: Some(false) });
            }
            let reactivated = journal.reactivate_if_drained(&binding).await?;
            Ok(Response {
                value: Some(reactivated),
            })
        }
        Request::Detached { binding, token } => {
            let marker = state_path
                .join("connection-drain")
                .join(binding.connection_id.as_str());
            if read_drain_token(&marker).as_deref() != Some(token.as_str())
                || connection_id_present(state, &binding)?
            {
                return Ok(Response { value: Some(false) });
            }
            let detached = journal.is_retired_and_drained(&binding).await?;
            Ok(Response {
                value: Some(
                    detached && read_drain_token(&marker).as_deref() == Some(token.as_str()),
                ),
            })
        }
    }
}

fn configured_exactly(
    state: &ControlState,
    binding: &RepositoryConnection,
) -> Result<bool, HostError> {
    let bindings = state.0.read().map_err(|_| HostError::Journal)?;
    Ok(bindings
        .iter()
        .any(|current| current.connection_id == binding.connection_id && current == binding))
}

fn connection_id_present(
    state: &ControlState,
    binding: &RepositoryConnection,
) -> Result<bool, HostError> {
    let bindings = state.0.read().map_err(|_| HostError::Journal)?;
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

fn read_request(stream: &mut UnixStream) -> Result<Request, HostError> {
    let length = read_frame_length(stream)?;
    if length == 0 || length > MAX_REQUEST_BYTES {
        return Err(HostError::Frame);
    }
    let mut bytes = vec![0; length];
    stream
        .read_exact(&mut bytes)
        .map_err(|_| HostError::Frame)?;
    serde_json::from_slice(&bytes).map_err(|_| HostError::Frame)
}

fn write_response(stream: &mut UnixStream, response: Response) -> Result<(), HostError> {
    let payload = response.encode().map_err(|()| HostError::Frame)?;
    stream
        .write_all(&encode_frame(&payload)?)
        .map_err(|_| HostError::Frame)
}

fn read_frame_length(stream: &mut UnixStream) -> Result<usize, HostError> {
    let mut prefix = [0; 4];
    stream
        .read_exact(&mut prefix)
        .map_err(|_| HostError::Frame)?;
    let length = usize::try_from(u32::from_be_bytes(prefix)).map_err(|_| HostError::Frame)?;
    if length > MAX_FRAME {
        return Err(HostError::Frame);
    }
    Ok(length)
}

fn trusted_peer(_stream: &UnixStream) -> bool {
    // The private parent directory and 0600 socket restrict connect access to
    // the state owner on macOS; Linux additionally validates SO_PEERCRED.
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsFd;

        let Ok(credentials) = rustix::net::sockopt::socket_peercred(_stream.as_fd()) else {
            return false;
        };
        return credentials.uid == rustix::process::geteuid();
    }
    #[cfg(not(target_os = "linux"))]
    true
}

fn private_state_directory(path: &Path) -> Result<PathBuf, HostError> {
    if !path.is_absolute() {
        return Err(HostError::Path);
    }
    fs::create_dir_all(path).map_err(|_| HostError::Path)?;
    let metadata = fs::symlink_metadata(path).map_err(|_| HostError::Path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(HostError::Path);
    }
    let owner = rustix::process::geteuid().as_raw();
    if metadata.uid() != owner {
        return Err(HostError::Path);
    }
    let path = fs::canonicalize(path).map_err(|_| HostError::Path)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).map_err(|_| HostError::Path)?;
    Ok(path)
}

fn remove_stale_socket(path: &Path) -> Result<(), HostError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_socket() && trusted_socket_owner(&metadata) => {
            fs::remove_file(path).map_err(|_| HostError::Path)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(_) | Err(_) => Err(HostError::Path),
    }
}

fn remove_owned_socket(path: &Path, identity: (u64, u64)) {
    if fs::symlink_metadata(path).is_ok_and(|metadata| {
        metadata.file_type().is_socket() && (metadata.dev(), metadata.ino()) == identity
    }) {
        drop(fs::remove_file(path));
    }
}

fn trusted_socket_owner(metadata: &fs::Metadata) -> bool {
    metadata.uid() == rustix::process::geteuid().as_raw()
}

#[cfg(test)]
#[path = "server_tests.rs"]
mod tests;

fn marker_exists(path: &Path) -> Result<bool, HostError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Ok(_) | Err(_) => Err(HostError::Path),
    }
}

pub(super) fn read_drain_token(path: &Path) -> Option<String> {
    use std::os::unix::fs::OpenOptionsExt;

    let mut options = fs::OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    let file = options.open(path).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > 64 {
        return None;
    }
    let mut token = String::new();
    file.take(65).read_to_string(&mut token).ok()?;
    valid_token(&token).then_some(token)
}

pub(super) fn valid_token(token: &str) -> bool {
    token.len() == 32
        && token
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
