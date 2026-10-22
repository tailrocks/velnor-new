//! Serve Docker test responses outside a blocked controller runtime.

use std::{
    io::{Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use bollard::Docker;

#[path = "threaded/state.rs"]
mod state;
use state::ServerState;

#[cfg(test)]
#[path = "threaded/tests.rs"]
mod tests;

use super::{
    DockerResponse, reason,
    request::{RequestFraming, RequestTrace},
};

pub(crate) struct BackgroundDockerStub {
    pub(crate) docker: Docker,
    path: PathBuf,
    stop: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<Result<Vec<String>, String>>>,
    expected: usize,
    observed: Arc<Mutex<Vec<RequestTrace>>>,
    state: Arc<ServerState>,
}

impl BackgroundDockerStub {
    pub(crate) fn open(responses: Vec<DockerResponse>) -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let number = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = PathBuf::from(format!(
            "/tmp/velnor-inspect-thread-{}-{number}.sock",
            std::process::id()
        ));
        let listener = UnixListener::bind(&path).map_err(|error| error.to_string())?;
        listener
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        let expected = responses.len();
        let observed = Arc::new(Mutex::new(Vec::new()));
        let thread_observed = Arc::clone(&observed);
        let state = Arc::new(ServerState::default());
        let thread_state = Arc::clone(&state);
        let (stop, stopped) = mpsc::channel();
        let server = thread::Builder::new()
            .name("velnor-docker-test-stub".to_owned())
            .spawn(move || {
                serve(
                    &listener,
                    responses,
                    &stopped,
                    &thread_observed,
                    &thread_state,
                )
            })
            .map_err(|error| error.to_string())?;
        let Some(socket) = path.to_str() else {
            state.request_stop();
            let _sent = stop.send(());
            let _joined = server.join();
            let _removed = std::fs::remove_file(&path).err().map(|error| error.kind());
            return Err("The Docker socket path is not valid UTF-8.".to_owned());
        };
        let docker = match Docker::connect_with_unix(socket, 120, bollard::API_DEFAULT_VERSION) {
            Ok(docker) => docker,
            Err(error) => {
                state.request_stop();
                let _sent = stop.send(());
                let _joined = server.join();
                let _removed = std::fs::remove_file(&path).err().map(|error| error.kind());
                return Err(error.to_string());
            }
        };
        Ok(Self {
            docker,
            path,
            stop: Some(stop),
            thread: Some(server),
            expected,
            observed,
            state,
        })
    }

    #[expect(
        clippy::unused_async_trait_impl,
        reason = "Keep the shared async finish interface used by both Docker test stubs."
    )]
    #[expect(
        clippy::unused_async,
        reason = "Keep the shared async finish interface used by both Docker test stubs."
    )]
    pub(crate) async fn finish_observed(mut self) -> Result<Vec<String>, String> {
        let stop = self
            .stop
            .take()
            .ok_or_else(|| "The Docker test server has already stopped.".to_owned())?;
        self.state.request_stop();
        let _sent = stop.send(());
        let server = self
            .thread
            .take()
            .ok_or_else(|| "The Docker test server has already stopped.".to_owned())?;
        let requests = server
            .join()
            .map_err(|_| "The Docker test server thread failed.".to_owned())??;
        std::fs::remove_file(&self.path).map_err(|error| error.to_string())?;
        if requests.len() != self.expected {
            return Err(format!(
                "The Docker test server expected {} requests but received {}.",
                self.expected,
                requests.len()
            ));
        }
        Ok(requests)
    }

    pub(crate) fn observed_request_summary(&self) -> Result<String, String> {
        self.observed
            .lock()
            .map(|requests| super::summarize_requests(&requests))
            .map_err(|_| "The test could not lock the Docker request trace.".to_owned())
    }
}

impl Drop for BackgroundDockerStub {
    fn drop(&mut self) {
        self.state.request_stop();
        if let Some(stop) = self.stop.take() {
            let _sent = stop.send(());
        }
        if let Some(server) = self.thread.take() {
            let _joined = server.join();
        }
        let _removed = std::fs::remove_file(&self.path)
            .err()
            .map(|error| error.kind());
    }
}

fn serve(
    listener: &UnixListener,
    responses: Vec<DockerResponse>,
    stopped: &mpsc::Receiver<()>,
    observed: &Mutex<Vec<RequestTrace>>,
    state: &ServerState,
) -> Result<Vec<String>, String> {
    let mut requests = Vec::with_capacity(responses.len());
    let mut responses = responses.into_iter();
    loop {
        if stopped.try_recv().is_ok() || state.is_stopping() {
            return Ok(requests);
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                normalize_accepted_stream(&stream).map_err(|error| error.to_string())?;
                if !state.register(&stream).map_err(|error| error.to_string())? {
                    return Ok(requests);
                }
                let line = match read_request(&mut stream) {
                    Ok(line) => line,
                    Err(failure) => {
                        state.clear_active();
                        if state.is_stopping() {
                            return Ok(requests);
                        }
                        observed
                            .lock()
                            .map_err(|_| {
                                "The test could not lock the Docker request trace.".to_owned()
                            })?
                            .push(RequestTrace {
                                path: failure.path,
                                response_index: None,
                                response_status: None,
                                failure: Some(failure.message.clone()),
                            });
                        return Err(failure.message);
                    }
                };
                requests.push(line.clone());
                let response = responses.next().unwrap_or(DockerResponse {
                    status: Some(500),
                    body: br#"{"message":"unexpected Docker request"}"#.to_vec(),
                    delay: Duration::ZERO,
                });
                let response_index = requests.len() - 1;
                if state.is_stopping() {
                    state.clear_active();
                    return Ok(requests);
                }
                observed
                    .lock()
                    .map_err(|_| "The test could not lock the Docker request trace.".to_owned())?
                    .push(RequestTrace {
                        path: Some(super::request::request_path(&line)?),
                        response_index: Some(response_index),
                        response_status: response.status,
                        failure: None,
                    });
                let response_result = write_response(&mut stream, &response);
                state.clear_active();
                response_result?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

fn normalize_accepted_stream(stream: &UnixStream) -> std::io::Result<()> {
    stream.set_nonblocking(false)
}

fn read_request(stream: &mut UnixStream) -> Result<String, super::request::RequestFailure> {
    let mut framing = RequestFraming::default();
    let mut buffer = [0_u8; 256];
    loop {
        let read = match stream.read(&mut buffer) {
            Ok(read) => read,
            Err(error) => return Err(framing.failure(error.to_string())),
        };
        if read == 0 {
            return framing
                .finish()
                .map(str::to_owned)
                .map_err(|error| framing.failure(error));
        }
        let complete = framing
            .push(&buffer[..read])
            .map_err(|error| framing.failure(error))?;
        if complete {
            return framing
                .finish()
                .map(str::to_owned)
                .map_err(|error| framing.failure(error));
        }
    }
}

fn write_response(stream: &mut UnixStream, response: &DockerResponse) -> Result<(), String> {
    thread::sleep(response.delay);
    let Some(status) = response.status else {
        return Ok(());
    };
    let message = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        reason(status),
        response.body.len()
    );
    stream
        .write_all(message.as_bytes())
        .and_then(|()| stream.write_all(&response.body))
        .map_err(|error| error.to_string())
}
