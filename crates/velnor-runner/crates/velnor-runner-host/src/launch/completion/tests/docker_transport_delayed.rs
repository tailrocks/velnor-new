use std::future::Future;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::task::Poll;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use bollard::Docker;

use crate::HostError;
use crate::journal_completion::CleanupClaim;

use super::*;

const IO_DEADLINE: Duration = Duration::from_secs(3);
const TASK_DEADLINE: Duration = Duration::from_secs(5);
const STOP_DRAIN: Duration = Duration::from_millis(50);
const CONTAINER_ID: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn delayed_docker_delete_holds_the_intent_until_its_response() -> Result<(), String> {
    let (scratch, journal) = open("completion-delayed-docker-delete").await?;
    let (id, runner_id, runner_name) = launch_unbound(&journal).await?;
    completion::record_completion_events(
        &journal,
        7,
        &completion_poll(117, runner_id, &runner_name),
    )
    .await
    .map_err(|error| error.to_string())?;
    let first = journal
        .claim_completion_cleanup_at(id, 100, 110)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "expected the first cleanup claim".to_owned())?;
    let mut docker_stub = DelayedDelete::open()?;
    let stale_effect =
        spawn_stale_effect(&journal, id, first.generation, docker_stub.docker.clone());
    let request = docker_stub.wait_for_delete()?;
    expire_claim(&journal, id).await?;
    let contender_task = spawn_waiting_contender(scratch.file(), id).await?;
    docker_stub.release_response()?;
    join_stale_effect(stale_effect).await?;
    let next = join_contender(contender_task).await?;
    assert_eq!(next.generation, first.generation + 1);

    let requests = docker_stub.finish()?;
    assert_eq!(requests, vec![request.clone()]);
    assert_delete_request(&request, CONTAINER_ID)?;
    Ok(())
}

async fn launch_unbound(journal: &Journal) -> Result<(i64, i64, String), String> {
    let crate::journal::LaunchReservation::New(id) = journal
        .reserve_assignment(7, 117, 1_117, 2)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("expected a new launch reservation".to_owned());
    };
    if !journal
        .claim_acquire(id)
        .await
        .map_err(|error| error.to_string())?
    {
        return Err("expected acquire reservation".to_owned());
    }
    journal
        .resolve_acquire(id, true)
        .await
        .map_err(|error| error.to_string())?;
    if !journal
        .claim_jit(id)
        .await
        .map_err(|error| error.to_string())?
    {
        return Err("expected JIT reservation".to_owned());
    }
    let identity = journal
        .launch_identity(id)
        .await
        .map_err(|error| error.to_string())?;
    let runner_id = 127;
    let runner_name = format!("v{}", identity.launch_id());
    journal
        .bind_github_runner(id, &runner_id.to_string())
        .await
        .map_err(|error| error.to_string())?;
    journal
        .finish(id, crate::journal::Outcome::Done)
        .await
        .map_err(|error| error.to_string())?;
    Ok((id, runner_id, runner_name))
}

fn spawn_stale_effect(
    journal: &Journal,
    id: i64,
    generation: i64,
    docker: Docker,
) -> tokio::task::JoinHandle<Result<Option<()>, HostError>> {
    let stale_journal = journal.clone();
    tokio::spawn(async move {
        stale_journal
            .run_completion_cleanup_effect_with_clock_for_test(
                id,
                generation,
                || Ok(105),
                move || async move {
                    crate::stage::PairEngine::remove(&docker, CONTAINER_ID)
                        .await
                        .map_err(|_| HostError::Docker)?;
                    Ok(())
                },
            )
            .await
    })
}

async fn spawn_waiting_contender(
    path: PathBuf,
    id: i64,
) -> Result<tokio::task::JoinHandle<Result<Option<CleanupClaim>, HostError>>, String> {
    let contender = crate::Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    contender
        .bind_engine("docker-engine-test")
        .await
        .map_err(|error| error.to_string())?;
    let contender_future = async move { contender.claim_completion_cleanup_at(id, 120, 130).await };
    let mut contender_future = Box::pin(contender_future);
    let mut immediate_result = None;
    let reached_lock_wait =
        std::future::poll_fn(|context| match contender_future.as_mut().poll(context) {
            Poll::Pending => Poll::Ready(true),
            Poll::Ready(result) => {
                immediate_result = Some(result);
                Poll::Ready(false)
            }
        })
        .await;
    assert!(
        reached_lock_wait,
        "the production cleanup claim must first wait on the held per-intent lock"
    );
    assert!(immediate_result.is_none());
    let mut task = tokio::spawn(contender_future);
    assert!(
        tokio::time::timeout(Duration::from_millis(150), &mut task)
            .await
            .is_err()
    );
    Ok(task)
}

async fn join_stale_effect(
    task: tokio::task::JoinHandle<Result<Option<()>, HostError>>,
) -> Result<(), String> {
    let result = tokio::time::timeout(TASK_DEADLINE, task)
        .await
        .map_err(|_| "stale Docker DELETE task exceeded its deadline".to_owned())?
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?;
    if result.is_none() {
        return Err("stale Docker DELETE effect lost its live claim".to_owned());
    }
    Ok(())
}

async fn join_contender(
    task: tokio::task::JoinHandle<Result<Option<CleanupClaim>, HostError>>,
) -> Result<CleanupClaim, String> {
    tokio::time::timeout(TASK_DEADLINE, task)
        .await
        .map_err(|_| "contender claim task exceeded its deadline".to_owned())?
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "expected a fresh cleanup claim after the response".to_owned())
}

fn assert_delete_request(request: &str, container_id: &str) -> Result<(), String> {
    let mut words = request.split_whitespace();
    if words.next() != Some("DELETE") {
        return Err(format!("expected a DELETE request, got {request:?}"));
    }
    let target = words
        .next()
        .ok_or_else(|| "Docker request target is missing".to_owned())?;
    if words.next() != Some("HTTP/1.1") || words.next().is_some() {
        return Err(format!("unexpected Docker request protocol: {request:?}"));
    }
    // Bollard 0.21.1 uses API_DEFAULT_VERSION 1.53, but its absolute Unix API
    // path join emits this exact unversioned route.
    let expected_target = format!("/containers/{container_id}?v=false&force=true&link=false");
    if target != expected_target {
        return Err(format!(
            "unexpected Docker API target: expected {expected_target:?}, got {target:?}"
        ));
    }
    Ok(())
}

async fn expire_claim(journal: &Journal, id: i64) -> Result<(), String> {
    journal
        .connection()
        .await
        .map_err(|error| error.to_string())?
        .execute(
            "UPDATE completion_cleanup SET lease_until = 0 WHERE intent_id = ?1",
            [id],
        )
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

struct DelayedDelete {
    docker: Docker,
    path: PathBuf,
    request: Receiver<String>,
    release: Option<Sender<()>>,
    stop: Option<Sender<()>>,
    task: Option<JoinHandle<Result<Vec<String>, String>>>,
}

impl DelayedDelete {
    fn open() -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let number = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = PathBuf::from(format!(
            "/tmp/velnor-delayed-delete-{}-{number}.sock",
            std::process::id()
        ));
        let listener = UnixListener::bind(&path).map_err(|error| error.to_string())?;
        listener
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        let (request_tx, request) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        let (stop, stop_rx) = mpsc::channel();
        let task = thread::spawn(move || serve_delete(listener, request_tx, release_rx, stop_rx));
        let socket = path
            .to_str()
            .ok_or_else(|| "Docker stub path is not UTF-8".to_owned())?;
        let docker = match Docker::connect_with_unix(socket, 120, bollard::API_DEFAULT_VERSION) {
            Ok(docker) => docker,
            Err(error) => {
                let _sent = release.send(());
                let _stopped = stop.send(());
                let _joined = task.join();
                let _removed = std::fs::remove_file(&path);
                return Err(error.to_string());
            }
        };
        Ok(Self {
            docker,
            path,
            request,
            release: Some(release),
            stop: Some(stop),
            task: Some(task),
        })
    }

    fn wait_for_delete(&self) -> Result<String, String> {
        self.request
            .recv_timeout(IO_DEADLINE)
            .map_err(|error| format!("Docker DELETE did not reach the stub: {error}"))
    }

    fn release_response(&mut self) -> Result<(), String> {
        self.release
            .take()
            .ok_or_else(|| "Docker DELETE response was already released".to_owned())?
            .send(())
            .map_err(|error| error.to_string())
    }

    fn finish(mut self) -> Result<Vec<String>, String> {
        self.stop
            .take()
            .ok_or_else(|| "Docker stub stop signal was already sent".to_owned())?
            .send(())
            .map_err(|error| error.to_string())?;
        let task = self
            .task
            .take()
            .ok_or_else(|| "Docker stub already stopped".to_owned())?;
        let result = task
            .join()
            .map_err(|_| "Docker stub server panicked".to_owned())?;
        let _removed = std::fs::remove_file(&self.path);
        result
    }
}

impl Drop for DelayedDelete {
    fn drop(&mut self) {
        if let Some(release) = self.release.take() {
            let _sent = release.send(());
        }
        if let Some(stop) = self.stop.take() {
            let _sent = stop.send(());
        }
        if let Some(task) = self.task.take() {
            let _joined = task.join();
        }
        let _removed = std::fs::remove_file(&self.path);
    }
}

fn serve_delete(
    listener: UnixListener,
    observed: Sender<String>,
    release: Receiver<()>,
    stop: Receiver<()>,
) -> Result<Vec<String>, String> {
    let deadline = Instant::now() + IO_DEADLINE;
    let mut requests = Vec::new();
    let mut stop_started = None;
    loop {
        if Instant::now() >= deadline {
            return Err("Docker client did not connect before the deadline".to_owned());
        }
        if stop.try_recv().is_ok() {
            stop_started = Some(Instant::now());
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream
                    .set_read_timeout(Some(IO_DEADLINE))
                    .map_err(|error| error.to_string())?;
                stream
                    .set_write_timeout(Some(IO_DEADLINE))
                    .map_err(|error| error.to_string())?;
                let request = read_request(&mut stream)?;
                observed
                    .send(request.clone())
                    .map_err(|error| error.to_string())?;
                if requests.is_empty() {
                    release
                        .recv_timeout(IO_DEADLINE)
                        .map_err(|error| format!("test did not release Docker DELETE: {error}"))?;
                }
                stream
                    .write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    .map_err(|error| error.to_string())?;
                requests.push(request);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if stop_started.is_some_and(|started: Instant| started.elapsed() >= STOP_DRAIN) {
                    return Ok(requests);
                }
                thread::sleep(Duration::from_millis(2));
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

fn read_request(stream: &mut UnixStream) -> Result<String, String> {
    const MAX_HEADERS: usize = 8192;
    let mut bytes = Vec::with_capacity(512);
    let mut buffer = [0_u8; 512];
    loop {
        let read = stream
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if read == 0 {
            return Err("Docker client closed before request headers".to_owned());
        }
        bytes.extend_from_slice(&buffer[..read]);
        if bytes.len() > MAX_HEADERS {
            return Err("Docker request headers exceed the bounded stub limit".to_owned());
        }
        if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            let headers = String::from_utf8(bytes).map_err(|error| error.to_string())?;
            return headers
                .lines()
                .next()
                .map(str::to_owned)
                .ok_or_else(|| "Docker request line is missing".to_owned());
        }
    }
}
