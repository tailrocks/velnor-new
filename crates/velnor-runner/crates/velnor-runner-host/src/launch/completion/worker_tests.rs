//! Completion worker signal coalescing, result propagation, and joining.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use bollard::Docker;
use tokio::sync::oneshot;

use super::{CompletionWorker, HostError, bounded_docker, wake};
use crate::docker_client::DOCKER_OPERATION_TIMEOUT;

#[test]
fn repeated_notifications_coalesce_to_one_bounded_wake() {
    let (sender, receiver) = mpsc::sync_channel(1);
    for _ in 0..32 {
        wake(&sender);
    }
    assert_eq!(receiver.try_recv(), Ok(()));
    assert_eq!(receiver.try_recv(), Err(mpsc::TryRecvError::Empty));
}

#[test]
fn completion_client_sets_a_request_bound_without_mutating_the_caller() -> Result<(), HostError> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "velnor-completion-client-{}-{id}.sock",
        std::process::id()
    ));
    std::fs::write(&path, b"").map_err(|_| HostError::Path)?;
    let socket = SocketFile(path);
    let socket_text = socket.0.to_str().ok_or(HostError::Path)?;
    let docker = Docker::connect_with_unix(socket_text, 120, bollard::API_DEFAULT_VERSION)
        .map_err(|_| HostError::Docker)?;
    let bounded = bounded_docker(docker.clone());
    assert_eq!(docker.timeout(), Duration::from_secs(120));
    assert_eq!(bounded.timeout(), DOCKER_OPERATION_TIMEOUT);
    Ok(())
}

#[tokio::test]
async fn shutdown_returns_worker_error_after_joining_thread() {
    let (worker, joined) = worker_with_result(Err(HostError::Endpoint), false);
    assert_eq!(worker.shutdown().await, Err(HostError::Endpoint));
    assert!(joined.load(Ordering::Acquire));
}

#[tokio::test]
async fn shutdown_reports_join_panic_after_receiving_worker_result() {
    let (worker, joined) = worker_with_result(Ok(()), true);
    assert_eq!(worker.shutdown().await, Err(HostError::Journal));
    assert!(joined.load(Ordering::Acquire));
}

fn worker_with_result(
    result: Result<(), HostError>,
    panic_after_result: bool,
) -> (CompletionWorker, Arc<AtomicBool>) {
    let stopping = Arc::new(AtomicBool::new(false));
    let thread_stopping = Arc::clone(&stopping);
    let joined = Arc::new(AtomicBool::new(false));
    let thread_joined = Arc::clone(&joined);
    let (wake, receiver) = mpsc::sync_channel(1);
    let (finished_tx, finished) = oneshot::channel();
    let thread = thread::spawn(move || {
        assert_eq!(receiver.recv_timeout(Duration::from_secs(1)), Ok(()));
        assert!(thread_stopping.load(Ordering::Acquire));
        assert!(finished_tx.send(result).is_ok());
        thread_joined.store(true, Ordering::Release);
        if panic_after_result {
            std::panic::panic_any("test join failure");
        }
    });
    (
        CompletionWorker {
            wake,
            stopping,
            thread: Some(thread),
            finished: Some(finished),
            reaped: None,
        },
        joined,
    )
}

struct SocketFile(PathBuf);

impl Drop for SocketFile {
    fn drop(&mut self) {
        match std::fs::remove_file(&self.0) {
            Ok(()) => {}
            Err(error) => {
                let _kind = error.kind();
            }
        }
    }
}
