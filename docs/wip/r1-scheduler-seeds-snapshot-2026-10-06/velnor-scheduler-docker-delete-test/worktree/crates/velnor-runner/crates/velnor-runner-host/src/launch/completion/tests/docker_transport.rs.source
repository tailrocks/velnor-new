use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::stage::PairEngine;
use bollard::Docker;

use super::*;

const SERVER_DEADLINE: Duration = Duration::from_secs(12);
const IO_DEADLINE: Duration = Duration::from_secs(3);

#[test]
fn cloned_docker_transport_finishes_cleanup_io_while_listener_runtime_is_blocked()
-> Result<(), String> {
    let listener = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    let (scratch, journal) = listener.block_on(open("completion-docker-runtime"))?;
    let (id, identity, runner_id, runner_name) = listener.block_on(launch_unbound(&journal))?;
    let stub = DockerStub::open(identity.engine_id(), identity.private_volume())?;
    listener
        .block_on(PairEngine::verify_engine(&stub.docker, &identity))
        .map_err(|error| format!("listener runtime could not warm Docker transport: {error}"))?;
    listener
        .block_on(completion::record_completion_events(
            &journal,
            7,
            &completion_poll(117, runner_id, &runner_name),
        ))
        .map_err(|error| error.to_string())?;
    let api = BlockingRunnerApi::released(&runner_name, runner_id);
    let tasks = listener
        .block_on(completion::schedule_completed_isolated(
            api,
            7,
            "admin-token",
            journal.clone(),
            stub.docker.clone(),
        ))
        .map_err(|error| error.to_string())?;
    if tasks.len() != 1 {
        return Err("cleanup did not claim one completed launch".to_owned());
    }
    stub.wait_for_cleanup_exchange()?;
    listener.block_on(join(tasks))?;
    let requests = stub.finish()?;
    assert!(
        requests.len() >= 20,
        "cleanup did not issue Docker requests"
    );
    assert!(is_info_request(&requests[0]));
    assert!(is_info_request(&requests[1]));
    assert!(
        requests
            .iter()
            .any(|line| line.contains("/containers/json"))
    );
    assert!(requests.iter().any(|line| line.contains("/volumes/")));
    let volume_deletes = requests
        .iter()
        .filter_map(|request| {
            let mut words = request.split_whitespace();
            let method = words.next()?;
            let target = words.next()?;
            (method == "DELETE").then(|| unversioned_path(target))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        volume_deletes,
        [
            format!("/volumes/{}-docker", identity.private_volume()),
            format!("/volumes/{}-work", identity.private_volume()),
            format!("/volumes/{}", identity.private_volume()),
        ]
    );
    assert!(
        listener
            .block_on(journal.intent(id))
            .map_err(|error| error.to_string())?
            .cleanup_proven
    );
    drop(scratch);
    Ok(())
}

async fn join(tasks: Vec<tokio::task::JoinHandle<()>>) -> Result<(), String> {
    for task in tasks {
        task.await.map_err(|error| error.to_string())?;
    }
    Ok(())
}

struct DockerStub {
    docker: Docker,
    path: PathBuf,
    stop: Option<Sender<()>>,
    first_cleanup: Receiver<()>,
    task: Option<JoinHandle<Result<Vec<String>, String>>>,
}

impl DockerStub {
    fn open(engine_id: &str, private_volume: &str) -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let number = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = PathBuf::from(format!(
            "/tmp/velnor-completion-docker-{}-{number}.sock",
            std::process::id()
        ));
        let listener = UnixListener::bind(&path).map_err(|error| error.to_string())?;
        listener
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        let (stop, stopped) = mpsc::channel();
        let (first_cleanup, first_cleanup_rx) = mpsc::channel();
        let engine_id = engine_id.to_owned();
        let private_volume = private_volume.to_owned();
        let task = thread::spawn(move || {
            serve(listener, stopped, first_cleanup, engine_id, private_volume)
        });
        let socket = path
            .to_str()
            .ok_or_else(|| "Docker stub path is not UTF-8".to_owned())?;
        let docker = match Docker::connect_with_unix(socket, 120, bollard::API_DEFAULT_VERSION) {
            Ok(docker) => docker,
            Err(error) => {
                let _sent = stop.send(());
                let _joined = task.join();
                let _removed = std::fs::remove_file(&path);
                return Err(error.to_string());
            }
        };
        Ok(Self {
            docker,
            path,
            stop: Some(stop),
            first_cleanup: first_cleanup_rx,
            task: Some(task),
        })
    }

    fn wait_for_cleanup_exchange(&self) -> Result<(), String> {
        self.first_cleanup
            .recv_timeout(IO_DEADLINE)
            .map_err(|error| {
                format!("cloned Docker request did not finish while listener was blocked: {error}")
            })
    }

    fn finish(mut self) -> Result<Vec<String>, String> {
        if let Some(stop) = self.stop.take() {
            stop.send(()).map_err(|error| error.to_string())?;
        }
        let task = self
            .task
            .take()
            .ok_or_else(|| "Docker stub already stopped".to_owned())?;
        let requests = task
            .join()
            .map_err(|_| "Docker stub server panicked".to_owned())??;
        std::fs::remove_file(&self.path).map_err(|error| error.to_string())?;
        Ok(requests)
    }
}

impl Drop for DockerStub {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _sent = stop.send(());
        }
        let _removed = std::fs::remove_file(&self.path);
    }
}

fn serve(
    listener: UnixListener,
    stop: Receiver<()>,
    first_cleanup: Sender<()>,
    engine_id: String,
    private_volume: String,
) -> Result<Vec<String>, String> {
    let deadline = Instant::now() + SERVER_DEADLINE;
    let mut requests = Vec::new();
    loop {
        if stopped(&stop)? {
            return Ok(requests);
        }
        if Instant::now() >= deadline {
            return Err("Docker stub exceeded its server deadline".to_owned());
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
                let (status, body) = response_for(&request, &engine_id, &private_volume)?;
                write_response(&mut stream, status, &body)?;
                requests.push(request);
                if requests.len() == 2 {
                    let _sent = first_cleanup.send(());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(2));
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

fn stopped(stop: &Receiver<()>) -> Result<bool, String> {
    match stop.try_recv() {
        Ok(()) | Err(TryRecvError::Disconnected) => Ok(true),
        Err(TryRecvError::Empty) => Ok(false),
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

fn response_for(
    request: &str,
    engine_id: &str,
    private_volume: &str,
) -> Result<(u16, String), String> {
    let mut words = request.split_whitespace();
    let method = words
        .next()
        .ok_or_else(|| "Docker method is missing".to_owned())?;
    let target = words
        .next()
        .ok_or_else(|| "Docker target is missing".to_owned())?;
    let path = unversioned_path(target);
    if method == "DELETE" {
        let volumes = [
            format!("/volumes/{private_volume}-docker"),
            format!("/volumes/{private_volume}-work"),
            format!("/volumes/{private_volume}"),
        ];
        if volumes.iter().any(|volume| volume == &path) {
            return Ok((404, r#"{"message":"not found"}"#.to_owned()));
        }
        return Err(format!("unexpected Docker DELETE target: {target}"));
    }
    if method != "GET" {
        return Err(format!("unexpected Docker method: {method}"));
    }
    if path.ends_with("/info") {
        Ok((200, format!(r#"{{"ID":"{engine_id}"}}"#)))
    } else if path.ends_with("/containers/json") {
        Ok((200, "[]".to_owned()))
    } else if path.starts_with("/containers/") && path.ends_with("/json") {
        Ok((404, r#"{"message":"not found"}"#.to_owned()))
    } else if path.starts_with("/volumes/") {
        Ok((404, r#"{"message":"not found"}"#.to_owned()))
    } else {
        Err(format!("unexpected Docker request target: {target}"))
    }
}

fn unversioned_path(target: &str) -> String {
    let path = target.split('?').next().unwrap_or_default();
    if let Some((version, rest)) = path.strip_prefix('/').and_then(|tail| tail.split_once('/')) {
        if version
            .strip_prefix('v')
            .is_some_and(|value| value.contains('.'))
        {
            return format!("/{rest}");
        }
    }
    path.to_owned()
}

fn is_info_request(request: &str) -> bool {
    request
        .split_whitespace()
        .nth(1)
        .is_some_and(|target| unversioned_path(target) == "/info")
}

fn write_response(stream: &mut UnixStream, status: u16, body: &str) -> Result<(), String> {
    let reason = if status == 200 { "OK" } else { "Not Found" };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(response.as_bytes())
        .map_err(|error| error.to_string())
}

async fn launch_unbound(journal: &Journal) -> Result<(i64, LaunchIdentity, i64, String), String> {
    let reservation = journal
        .reserve_assignment(7, 117, 1_117, 2)
        .await
        .map_err(|error| error.to_string())?;
    let crate::journal::LaunchReservation::New(id) = reservation else {
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
    let recorded = journal
        .launch_identity(id)
        .await
        .map_err(|error| error.to_string())?;
    let runner_id = 127;
    let runner_name = format!("v{}", recorded.launch_id());
    journal
        .bind_github_runner(id, &runner_id.to_string())
        .await
        .map_err(|error| error.to_string())?;
    journal
        .finish(id, Outcome::Done)
        .await
        .map_err(|error| error.to_string())?;
    Ok((id, recorded, runner_id, runner_name))
}
