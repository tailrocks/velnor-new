use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use tokio::sync::OwnedSemaphorePermit;
use velnor_runner_github::{Exchange, TransportFail};
use zeroize::{Zeroize, Zeroizing};

use super::curl_args;
use super::readers::{BodyReadError, parse_http_status, read_bounded, read_tail};

#[path = "discovery_curl_wait.rs"]
mod wait;
use wait::{join_finished, receive_result, thread_done, wait_for_exit};

#[path = "discovery_curl_reaper.rs"]
mod reaper;
use reaper::{ReapResources, defer_reap};

const CLEANUP_BUDGET: Duration = Duration::from_millis(250);

pub(super) struct CurlChild {
    child: Option<Child>,
    body_rx: Receiver<Result<Vec<u8>, BodyReadError>>,
    status_rx: Receiver<Result<Vec<u8>, BodyReadError>>,
    config_rx: Option<Receiver<Result<(), ()>>>,
    config_written: bool,
    config_thread: Option<JoinHandle<()>>,
    body_thread: Option<JoinHandle<()>>,
    status_thread: Option<JoinHandle<()>>,
    cleanup_deadline: Instant,
    permit: Option<Arc<OwnedSemaphorePermit>>,
}

impl CurlChild {
    pub(super) fn spawn(
        executable: &str,
        body_limit: usize,
        cleanup_deadline: Instant,
        permit: Arc<OwnedSemaphorePermit>,
    ) -> Result<Self, TransportFail> {
        let mut command = Command::new(executable);
        command
            .args(curl_args())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let child = command.spawn().map_err(|_| TransportFail::Reset)?;
        let (body_tx, body_rx) = mpsc::sync_channel(1);
        let (status_tx, status_rx) = mpsc::sync_channel(1);
        let mut process = Self {
            child: Some(child),
            body_rx,
            status_rx,
            config_rx: None,
            config_written: false,
            config_thread: None,
            body_thread: None,
            status_thread: None,
            cleanup_deadline,
            permit: Some(permit),
        };
        let stdout = process
            .child
            .as_mut()
            .and_then(|child| child.stdout.take())
            .ok_or(TransportFail::Reset)?;
        let stderr = process
            .child
            .as_mut()
            .and_then(|child| child.stderr.take())
            .ok_or(TransportFail::Reset)?;
        process.body_thread = Some(
            thread::Builder::new()
                .name("velnor-http-body".to_owned())
                .spawn(move || {
                    let result = read_bounded(stdout, body_limit);
                    let _sent = body_tx.send(result);
                })
                .map_err(|_| TransportFail::Reset)?,
        );
        process.status_thread = Some(
            thread::Builder::new()
                .name("velnor-http-status".to_owned())
                .spawn(move || {
                    let result = read_tail(stderr, 128);
                    let _sent = status_tx.send(result);
                })
                .map_err(|_| TransportFail::Reset)?,
        );
        Ok(process)
    }

    pub(super) fn start_config(
        &mut self,
        config: Zeroizing<String>,
        stop_at: Instant,
        cancellation: &AtomicBool,
    ) -> Result<(), TransportFail> {
        if cancellation.load(Ordering::Acquire) {
            return Err(TransportFail::Reset);
        }
        if Instant::now() >= stop_at {
            return Err(TransportFail::Timeout);
        }
        let stdin = self
            .child
            .as_mut()
            .and_then(|child| child.stdin.take())
            .ok_or(TransportFail::Reset)?;
        let (sender, receiver) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("velnor-http-config".to_owned())
            .spawn(move || {
                let mut stdin = stdin;
                let result = stdin.write_all(config.as_bytes()).map_err(|_| ());
                drop(stdin);
                let _sent = sender.send(result);
            })
            .map_err(|_| TransportFail::Reset)?;
        self.config_rx = Some(receiver);
        self.config_thread = Some(thread);
        Ok(())
    }

    pub(super) fn finish(
        mut self,
        stop_at: Instant,
        cancellation: &AtomicBool,
    ) -> Result<Exchange, TransportFail> {
        let config_rx = self.config_rx.as_ref().ok_or(TransportFail::Reset)?;
        let child = self.child.as_mut().ok_or(TransportFail::Reset)?;
        let (exit, mut body) = wait_for_exit(
            child,
            &self.body_rx,
            config_rx,
            &mut self.config_written,
            stop_at,
            cancellation,
        )?;
        let mut body = match body.take() {
            Some(body) => body,
            None => receive_result(&self.body_rx, stop_at, cancellation)?,
        };
        let mut tail = match receive_result(&self.status_rx, stop_at, cancellation) {
            Ok(tail) => tail,
            Err(error) => {
                body.zeroize();
                return Err(error);
            }
        };
        if self.join_threads_until(stop_at).is_err() {
            body.zeroize();
            tail.zeroize();
            return Err(TransportFail::Timeout);
        }
        if Instant::now() >= stop_at {
            body.zeroize();
            tail.zeroize();
            return Err(TransportFail::Timeout);
        }
        let Some(code) = exit.code() else {
            body.zeroize();
            tail.zeroize();
            return Err(TransportFail::Reset);
        };
        if code == 28 {
            body.zeroize();
            tail.zeroize();
            return Err(TransportFail::Timeout);
        }
        if code != 0 {
            body.zeroize();
            tail.zeroize();
            return Err(TransportFail::Reset);
        }
        let Some(status) = parse_http_status(&tail) else {
            body.zeroize();
            tail.zeroize();
            return Err(TransportFail::Reset);
        };
        tail.zeroize();
        if Instant::now() >= stop_at {
            body.zeroize();
            return Err(TransportFail::Timeout);
        }
        Ok(Exchange { status, body })
    }

    fn join_threads_until(&mut self, stop_at: Instant) -> Result<(), TransportFail> {
        loop {
            let config_done = join_finished(&mut self.config_thread)?;
            let body_done = join_finished(&mut self.body_thread)?;
            let status_done = join_finished(&mut self.status_thread)?;
            if config_done && body_done && status_done {
                return Ok(());
            }
            if Instant::now() >= stop_at {
                return Err(TransportFail::Timeout);
            }
            thread::sleep(Duration::from_millis(2));
        }
    }

    fn is_quiescent(&mut self) -> bool {
        let child_state = poll_child(self.child.as_mut());
        child_state == ChildState::Reaped
            && thread_done(self.config_thread.as_ref())
            && thread_done(self.body_thread.as_ref())
            && thread_done(self.status_thread.as_ref())
    }

    fn kill_owned_child(&mut self) {
        let child_state = poll_child(self.child.as_mut());
        if child_state != ChildState::Running {
            return;
        }
        if let Some(child) = self.child.as_mut() {
            let _killed = child.kill();
        }
    }

    fn join_finished_threads(&mut self) {
        let _config_joined = join_finished(&mut self.config_thread).is_ok();
        let _body_joined = join_finished(&mut self.body_thread).is_ok();
        let _status_joined = join_finished(&mut self.status_thread).is_ok();
    }

    fn zeroize_pending_output(&self) {
        if let Ok(Ok(mut body)) = self.body_rx.try_recv() {
            body.zeroize();
        }
        if let Ok(Ok(mut status)) = self.status_rx.try_recv() {
            status.zeroize();
        }
    }

    fn defer_unresolved(&mut self) {
        let Some(child) = self.child.take() else {
            return;
        };
        defer_reap(ReapResources {
            child,
            body_rx: std::mem::replace(&mut self.body_rx, mpsc::sync_channel(1).1),
            status_rx: std::mem::replace(&mut self.status_rx, mpsc::sync_channel(1).1),
            config_thread: self.config_thread.take(),
            body_thread: self.body_thread.take(),
            status_thread: self.status_thread.take(),
            _permit: self.permit.take(),
        });
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum ChildState {
    Running,
    Reaped,
    Unknown,
}

pub(super) fn poll_child(child: Option<&mut Child>) -> ChildState {
    let Some(child) = child else {
        return ChildState::Unknown;
    };
    match child.try_wait() {
        Ok(None) => ChildState::Running,
        Ok(Some(_)) => ChildState::Reaped,
        Err(_) => ChildState::Unknown,
    }
}

impl Drop for CurlChild {
    fn drop(&mut self) {
        let cleanup_end = Instant::now()
            .checked_add(CLEANUP_BUDGET)
            .unwrap_or(self.cleanup_deadline)
            .min(self.cleanup_deadline);
        while !self.is_quiescent() && Instant::now() < cleanup_end {
            self.kill_owned_child();
            thread::sleep(Duration::from_millis(2));
        }
        if self.is_quiescent() {
            self.join_finished_threads();
            self.zeroize_pending_output();
        } else {
            self.kill_owned_child();
            self.defer_unresolved();
        }
    }
}
