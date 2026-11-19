// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::AtomicBool;

use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::thread::{self, JoinHandle};

use tokio::sync::oneshot;

use crate::error::HostError;

use super::runner_log_state::CaptureStateRequest;
use super::runner_logs::RunnerLogStageRequest;
use super::stage::{OperationKey, StageRequest, write_request};

static WORKER: OnceLock<Result<Worker, HostError>> = OnceLock::new();

pub(super) struct Worker {
    sender: mpsc::SyncSender<Job>,
    active: Arc<Mutex<Option<OperationKey>>>,
    _thread: JoinHandle<()>,
}

struct Job {
    request: WriteRequest,
    cancelled: Arc<AtomicBool>,
    reply: oneshot::Sender<Result<(), HostError>>,
}

enum WriteRequest {
    Record(StageRequest),
    RunnerLogs(RunnerLogStageRequest),
    CaptureState(CaptureStateRequest),
}

impl WriteRequest {
    fn key(&self) -> &OperationKey {
        match self {
            Self::Record(request) => &request.key,
            Self::RunnerLogs(request) => request.key(),
            Self::CaptureState(request) => request.key(),
        }
    }

    fn write(self, cancelled: &AtomicBool) -> Result<(), HostError> {
        match self {
            Self::Record(request) => write_request(request, cancelled),
            Self::RunnerLogs(request) => request.write_request(cancelled),
            Self::CaptureState(request) => request.write_request(cancelled),
        }
    }
}

pub(super) fn process_worker() -> Result<&'static Worker, HostError> {
    WORKER
        .get_or_init(Worker::start)
        .as_ref()
        .map_err(|_| HostError::Journal)
}

impl Worker {
    fn start() -> Result<Self, HostError> {
        let (sender, receiver) = mpsc::sync_channel::<Job>(1);
        let active = Arc::new(Mutex::new(None));
        let thread_active = Arc::clone(&active);
        let thread = thread::Builder::new()
            .name("velnor-diagnostics-writer".to_owned())
            .spawn(move || worker_loop(&receiver, &thread_active))
            .map_err(|_| HostError::Journal)?;
        Ok(Self {
            sender,
            active,
            _thread: thread,
        })
    }

    pub(super) fn pending_for(
        &self,
        journal_identity: &str,
        connection_id: &str,
        intent_id: i64,
    ) -> Result<bool, HostError> {
        let active = self.active.lock().map_err(|_| HostError::Lock)?;
        Ok(active.as_ref().is_some_and(|key| {
            key.journal_identity == journal_identity
                && key.connection_id == connection_id
                && key.intent_id == intent_id
        }))
    }

    pub(super) fn submit(
        &self,
        request: StageRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<oneshot::Receiver<Result<(), HostError>>, HostError> {
        self.submit_request(WriteRequest::Record(request), cancelled)
    }

    pub(super) fn submit_runner_logs(
        &self,
        request: RunnerLogStageRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<oneshot::Receiver<Result<(), HostError>>, HostError> {
        self.submit_request(WriteRequest::RunnerLogs(request), cancelled)
    }

    pub(super) fn submit_capture_state(
        &self,
        request: CaptureStateRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<oneshot::Receiver<Result<(), HostError>>, HostError> {
        self.submit_request(WriteRequest::CaptureState(request), cancelled)
    }

    fn submit_request(
        &self,
        request: WriteRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<oneshot::Receiver<Result<(), HostError>>, HostError> {
        let (reply, receive) = oneshot::channel();
        let key = request.key().clone();
        let mut active = self.active.lock().map_err(|_| HostError::Lock)?;
        if active.is_some() {
            return Err(HostError::Journal);
        }
        *active = Some(key);
        if self
            .sender
            .try_send(Job {
                request,
                cancelled,
                reply,
            })
            .is_ok()
        {
            Ok(receive)
        } else {
            *active = None;
            Err(HostError::Journal)
        }
    }
}

fn worker_loop(receiver: &mpsc::Receiver<Job>, active: &Arc<Mutex<Option<OperationKey>>>) {
    while let Ok(job) = receiver.recv() {
        let key = job.request.key().clone();

        let result = catch_unwind(AssertUnwindSafe(|| job.request.write(&job.cancelled)))
            .unwrap_or(Err(HostError::Path));
        if let Ok(mut current) = active.lock()
            && current.as_ref() == Some(&key)
        {
            *current = None;
        }
        let _ignored = job.reply.send(result);
    }
}
