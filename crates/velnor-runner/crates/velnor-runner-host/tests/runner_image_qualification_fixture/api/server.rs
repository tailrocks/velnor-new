// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::http::{read_request, write_reply};
use super::routes::route;
use super::state::{ApiPlan, Snapshot, State};

pub(crate) struct DockerApiFixture {
    _temp: tempfile::TempDir,
    socket: PathBuf,
    state: Arc<Mutex<State>>,
    stopping: Arc<AtomicBool>,
    server: Option<JoinHandle<()>>,
}

impl DockerApiFixture {
    pub(crate) fn new(plan: ApiPlan) -> Result<Self, Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let socket = temp.path().join("docker.sock");
        let listener = UnixListener::bind(&socket)?;
        listener.set_nonblocking(true)?;
        let state = Arc::new(Mutex::new(State::new(plan)));
        let stopping = Arc::new(AtomicBool::new(false));
        let server = run_server(listener, Arc::clone(&state), Arc::clone(&stopping));
        Ok(Self {
            _temp: temp,
            socket,
            state,
            stopping,
            server: Some(server),
        })
    }

    pub(crate) fn socket(&self) -> &Path {
        &self.socket
    }

    pub(crate) fn snapshot(&self) -> std::io::Result<Snapshot> {
        let state = self
            .state
            .lock()
            .map_err(|_| std::io::Error::other("Docker API fixture state was poisoned"))?;
        Ok(state.snapshot())
    }
}

impl Drop for DockerApiFixture {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        drop(UnixStream::connect(&self.socket));
        if let Some(server) = self.server.take() {
            drop(server.join());
        }
    }
}

fn run_server(
    listener: UnixListener,
    state: Arc<Mutex<State>>,
    stopping: Arc<AtomicBool>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut workers = Vec::new();
        while !stopping.load(Ordering::Acquire) {
            match listener.accept() {
                Ok((stream, _)) => {
                    let state = Arc::clone(&state);
                    workers.push(thread::spawn(move || serve(stream, &state)));
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(_) => break,
            }
        }
        for worker in workers {
            drop(worker.join());
        }
    })
}

fn serve(mut stream: UnixStream, state: &Arc<Mutex<State>>) {
    if configure_stream(&stream).is_err() {
        return;
    }
    let Ok(request) = read_request(&mut stream) else {
        return;
    };
    let reply = route(&request, state);
    drop(write_reply(&mut stream, &reply));
}

fn configure_stream(stream: &UnixStream) -> std::io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))
}
