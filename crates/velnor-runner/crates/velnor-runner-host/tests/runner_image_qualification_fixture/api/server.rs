// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::http::{read_request, write_reply};
use super::routes::route;
use super::state::{ApiPlan, Snapshot, State};

pub(crate) struct DockerApiFixture {
    _temp: tempfile::TempDir,
    socket: PathBuf,
    state: Arc<Mutex<State>>,
    settled: Arc<Condvar>,
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
        let settled = Arc::new(Condvar::new());
        let stopping = Arc::new(AtomicBool::new(false));
        let server = run_server(
            listener,
            Arc::clone(&state),
            Arc::clone(&settled),
            Arc::clone(&stopping),
        );
        Ok(Self {
            _temp: temp,
            socket,
            state,
            settled,
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

    pub(crate) fn settled_snapshot(&self, timeout: Duration) -> std::io::Result<Snapshot> {
        let end = Instant::now() + timeout;
        let mut state = self
            .state
            .lock()
            .map_err(|_| std::io::Error::other("Docker API fixture state was poisoned"))?;
        while state.pending_mutations > 0 {
            let remaining = end.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "Docker API fixture mutation did not settle",
                ));
            }
            let (next, waited) = self
                .settled
                .wait_timeout(state, remaining)
                .map_err(|_| std::io::Error::other("Docker API fixture state was poisoned"))?;
            state = next;
            if waited.timed_out() && state.pending_mutations > 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "Docker API fixture mutation did not settle",
                ));
            }
        }
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
    settled: Arc<Condvar>,
    stopping: Arc<AtomicBool>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut workers = Vec::new();
        while !stopping.load(Ordering::Acquire) {
            match listener.accept() {
                Ok((stream, _)) => {
                    let state = Arc::clone(&state);
                    let settled = Arc::clone(&settled);
                    let stopping = Arc::clone(&stopping);
                    workers.push(thread::spawn(move || {
                        serve(stream, &state, &settled, &stopping);
                    }));
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

fn serve(
    mut stream: UnixStream,
    state: &Arc<Mutex<State>>,
    settled: &Condvar,
    stopping: &AtomicBool,
) {
    if configure_stream(&stream).is_err() {
        return;
    }
    let Ok(request) = read_request(&mut stream) else {
        return;
    };
    let mut reply = route(&request, state);
    if reply.disconnect
        && let (Some(delay), Some(mutation)) =
            (reply.deferred_delay.take(), reply.deferred_mutation.take())
    {
        drop(write_reply(&mut stream, &reply));
        drop(stream);
        if sleep_unless_stopping(delay, stopping) {
            apply_deferred(state, settled, mutation);
        }
        return;
    }
    if !reply.upgraded {
        if let Some(delay) = reply.delay {
            if !sleep_unless_stopping(delay, stopping) {
                return;
            }
            reply.delay = None;
        }
        if let Some(mutation) = reply.deferred_mutation.take() {
            apply_deferred(state, settled, mutation);
        }
    }
    drop(write_reply(&mut stream, &reply));
}

fn apply_deferred(
    state: &Arc<Mutex<State>>,
    settled: &Condvar,
    mutation: super::state::DeferredMutation,
) {
    let Ok(mut state) = state.lock() else {
        return;
    };
    state.apply_deferred(mutation);
    drop(state);
    settled.notify_all();
}

fn sleep_unless_stopping(duration: Duration, stopping: &AtomicBool) -> bool {
    let end = Instant::now() + duration;
    loop {
        if stopping.load(Ordering::Acquire) {
            return false;
        }
        let remaining = end.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return true;
        }
        thread::sleep(Duration::from_millis(10).min(remaining));
    }
}

fn configure_stream(stream: &UnixStream) -> std::io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))
}
