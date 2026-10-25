use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::AtomicBool;
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::thread::{self, JoinHandle};

use tokio::sync::oneshot;

use crate::error::HostError;

use super::runner_log_state::CaptureStateRequest;
use super::runner_logs::RunnerLogStageRequest;
use super::stage::{OperationKey, StageRequest, write_request};

static WORKER: OnceLock<Result<Worker, HostError>> = OnceLock::new();
#[cfg(test)]
static RUNNER_LOG_SUBMISSIONS: AtomicUsize = AtomicUsize::new(0);
#[cfg(test)]
static FAIL_RUNNER_LOG_WRITES: AtomicUsize = AtomicUsize::new(0);

#[cfg(test)]
pub(in crate::launch) fn runner_log_submission_count() -> usize {
    RUNNER_LOG_SUBMISSIONS.load(Ordering::Relaxed)
}

#[cfg(test)]
pub(in crate::launch) fn fail_next_runner_log_writes(count: usize) {
    FAIL_RUNNER_LOG_WRITES.store(count, Ordering::Relaxed);
}

pub(super) struct Worker {
    sender: mpsc::SyncSender<Job>,
    active: Arc<Mutex<Option<OperationKey>>>,
    _thread: JoinHandle<()>,
}

struct Job {
    request: WriteRequest,
    cancelled: Arc<AtomicBool>,
    reply: oneshot::Sender<Result<(), HostError>>,
    #[cfg(test)]
    gate: Option<Arc<std::sync::Barrier>>,
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
                #[cfg(test)]
                gate: None,
            })
            .is_ok()
        {
            Ok(receive)
        } else {
            *active = None;
            Err(HostError::Journal)
        }
    }

    #[cfg(test)]
    pub(super) fn submit_gated(
        &self,
        request: StageRequest,
        cancelled: Arc<AtomicBool>,
        gate: Arc<std::sync::Barrier>,
    ) -> Result<oneshot::Receiver<Result<(), HostError>>, HostError> {
        let (reply, receive) = oneshot::channel();
        let key = request.key.clone();
        let mut active = self.active.lock().map_err(|_| HostError::Lock)?;
        if active.is_some() {
            return Err(HostError::Journal);
        }
        *active = Some(key);
        if self
            .sender
            .try_send(Job {
                request: WriteRequest::Record(request),
                cancelled,
                reply,
                gate: Some(gate),
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
        #[cfg(test)]
        if let Some(gate) = &job.gate {
            gate.wait();
        }
        #[cfg(test)]
        let result = if matches!(&job.request, WriteRequest::RunnerLogs(_)) {
            RUNNER_LOG_SUBMISSIONS.fetch_add(1, Ordering::Relaxed);
            if take_forced_runner_log_failure() {
                Err(HostError::Path)
            } else {
                catch_unwind(AssertUnwindSafe(|| job.request.write(&job.cancelled)))
                    .unwrap_or(Err(HostError::Path))
            }
        } else {
            catch_unwind(AssertUnwindSafe(|| job.request.write(&job.cancelled)))
                .unwrap_or(Err(HostError::Path))
        };
        #[cfg(not(test))]
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

#[cfg(test)]
fn take_forced_runner_log_failure() -> bool {
    FAIL_RUNNER_LOG_WRITES
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
            count.checked_sub(1)
        })
        .is_ok()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::fs::File;
    use std::os::fd::AsFd;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Barrier;
    use std::sync::atomic::{AtomicBool, AtomicU64};

    use super::{OperationKey, StageRequest, Worker};

    fn request(
        root: &std::path::Path,
        parent: &std::path::Path,
        scope: &std::path::Path,
        connection: &str,
    ) -> Result<StageRequest, String> {
        let state_file = File::open(root).map_err(|error| error.to_string())?;
        let parent_file = File::open(parent).map_err(|error| error.to_string())?;
        let scope_file = File::open(scope).map_err(|error| error.to_string())?;
        let state = state_file
            .as_fd()
            .try_clone_to_owned()
            .map_err(|error| error.to_string())?;
        let parent = parent_file
            .as_fd()
            .try_clone_to_owned()
            .map_err(|error| error.to_string())?;
        let scope = scope_file
            .as_fd()
            .try_clone_to_owned()
            .map_err(|error| error.to_string())?;
        Ok(StageRequest {
            state,
            parent,
            scope,
            key: OperationKey {
                journal_identity: "test-journal".to_owned(),
                connection_id: connection.to_owned(),
                intent_id: 1,
            },
            generation: 1,
            role: "runner".to_owned(),
            container_id: "owned-runner".to_owned(),
            bytes: b"typed-record".to_vec(),
            started: std::time::Instant::now(),
        })
    }

    #[tokio::test]
    async fn one_global_worker_slot_covers_every_connection_until_io_finishes() -> Result<(), String>
    {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-diagnostics-worker-{}-{id}",
            std::process::id()
        ));
        fs::create_dir(&path).map_err(|error| error.to_string())?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
        let parent = path.join("completion-diagnostics");
        let scope = parent.join("scope");
        fs::create_dir(&parent).map_err(|error| error.to_string())?;
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
        fs::create_dir(&scope).map_err(|error| error.to_string())?;
        fs::set_permissions(&scope, fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
        let worker = Worker::start().map_err(|error| error.to_string())?;
        let gate = std::sync::Arc::new(Barrier::new(2));
        let reply = worker
            .submit_gated(
                request(&path, &parent, &scope, "essential")?,
                std::sync::Arc::new(AtomicBool::new(false)),
                std::sync::Arc::clone(&gate),
            )
            .map_err(|error| error.to_string())?;
        if worker
            .submit(
                request(&path, &parent, &scope, "chainargos")?,
                std::sync::Arc::new(AtomicBool::new(false)),
            )
            .is_ok()
        {
            return Err("second connection used the active global slot".to_owned());
        }
        if !worker
            .pending_for("test-journal", "essential", 1)
            .map_err(|error| error.to_string())?
            || worker
                .pending_for("test-journal", "chainargos", 1)
                .map_err(|error| error.to_string())?
        {
            return Err("worker slot did not retain its exact owner".to_owned());
        }
        gate.wait();
        reply
            .await
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())?;
        if worker
            .pending_for("test-journal", "essential", 1)
            .map_err(|error| error.to_string())?
        {
            return Err("worker slot remained busy after I/O ended".to_owned());
        }
        fs::remove_dir_all(path).map_err(|error| error.to_string())?;
        Ok(())
    }

    #[tokio::test]
    async fn delayed_diagnostics_fsync_keeps_the_global_slot_until_io_ends() -> Result<(), String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        if !std::env::var("DYLD_INSERT_LIBRARIES")
            .unwrap_or_default()
            .contains("fsync_delay")
        {
            return Ok(());
        }
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-diagnostics-delayed-worker-{}-{id}",
            std::process::id()
        ));
        let parent = path.join("completion-diagnostics");
        let scope = parent.join("scope");
        fs::create_dir_all(&scope).map_err(|error| error.to_string())?;
        for directory in [&path, &parent, &scope] {
            fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
                .map_err(|error| error.to_string())?;
        }
        let worker = Worker::start().map_err(|error| error.to_string())?;
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let mut reply = worker
            .submit(
                request(&path, &parent, &scope, "essential")?,
                std::sync::Arc::clone(&cancel),
            )
            .map_err(|error| error.to_string())?;
        if tokio::time::timeout(std::time::Duration::from_secs(15), &mut reply)
            .await
            .is_ok()
        {
            return Err("delayed write returned before the capture deadline".to_owned());
        }
        cancel.store(true, std::sync::atomic::Ordering::Release);
        if !worker
            .pending_for("test-journal", "essential", 1)
            .map_err(|error| error.to_string())?
        {
            return Err("timeout released the slot while the file write was blocked".to_owned());
        }
        let result = reply.await.map_err(|error| error.to_string())?;
        if result != Err(crate::error::HostError::DockerTimeout) {
            return Err("late file completion did not report a timeout".to_owned());
        }
        fs::remove_dir_all(path).map_err(|error| error.to_string())?;
        Ok(())
    }
}
