// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mutation {
    None,
    Repository,
    App,
    Feedback,
    Source,
    Checks,
    Duplicate,
    Pagination,
    Redirect,
}

#[derive(Default)]
struct Counts {
    api: AtomicUsize,
    storage: AtomicUsize,
    bad_auth: AtomicBool,
    leaked: AtomicBool,
    triggered: AtomicBool,
}

pub(super) struct Server {
    pub(super) api: String,
    pub(super) storage: String,
    stop: Arc<AtomicBool>,
    counts: Arc<Counts>,
    workers: Vec<JoinHandle<()>>,
}

impl Server {
    pub(super) fn start(api: Api, mutation: Mutation) -> Result<Self, Box<dyn std::error::Error>> {
        let api_listener = TcpListener::bind("127.0.0.1:0")?;
        let storage_listener = TcpListener::bind("127.0.0.1:0")?;
        api_listener.set_nonblocking(true)?;
        storage_listener.set_nonblocking(true)?;
        let api_origin = format!("http://{}", api_listener.local_addr()?);
        let storage_origin = format!("http://{}", storage_listener.local_addr()?);
        let stop = Arc::new(AtomicBool::new(false));
        let counts = Arc::new(Counts::default());
        let archives = api.archives.clone();
        let log = api.log.clone();
        let worker_api = spawn(api_listener, stop.clone(), {
            let counts = counts.clone();
            let storage = storage_origin.clone();
            let api_origin = api_origin.clone();
            let mut state = State::new(api, mutation, counts.clone());
            move |request| {
                counts.api.fetch_add(1, Ordering::SeqCst);
                if !request.authorized {
                    counts.bad_auth.store(true, Ordering::SeqCst);
                }
                state.respond(&request, &storage, &api_origin)
            }
        });
        let worker_storage = spawn(storage_listener, stop.clone(), {
            let counts = counts.clone();
            move |request| {
                counts.storage.fetch_add(1, Ordering::SeqCst);
                if request.authorized || request.has_authorization {
                    counts.leaked.store(true, Ordering::SeqCst);
                }
                let bytes = if request.path == "/storage/log" {
                    Some(log.as_slice())
                } else {
                    request
                        .path
                        .strip_prefix("/storage/a")
                        .and_then(|id| id.parse::<u64>().ok())
                        .and_then(|id| archives.get(&id).map(Vec::as_slice))
                };
                bytes.map_or_else(
                    || Response::status(404),
                    |body| Response::body(body.to_vec()),
                )
            }
        });
        Ok(Self {
            api: api_origin,
            storage: storage_origin,
            stop,
            counts,
            workers: vec![worker_api, worker_storage],
        })
    }

    pub(super) fn assert_authentication(&self, expect_storage: bool) {
        assert!(self.counts.api.load(Ordering::SeqCst) > 0);
        assert!(!self.counts.bad_auth.load(Ordering::SeqCst));
        assert!(!self.counts.leaked.load(Ordering::SeqCst));
        if expect_storage {
            assert!(self.counts.storage.load(Ordering::SeqCst) > 0);
        }
    }

    pub(super) fn assert_mutation(&self) {
        assert!(self.counts.triggered.load(Ordering::SeqCst));
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        for worker in self.workers.drain(..) {
            worker.join().expect("fixture server");
        }
    }
}

struct Request {
    path: String,
    body: Vec<u8>,
    authorized: bool,
    has_authorization: bool,
}
struct Response {
    status: u16,
    body: Vec<u8>,
    location: Option<String>,
}
impl Response {
    fn status(status: u16) -> Self {
        Self {
            status,
            body: Vec::new(),
            location: None,
        }
    }
    fn body(body: Vec<u8>) -> Self {
        Self {
            status: 200,
            body,
            location: None,
        }
    }
    fn json(value: &Value) -> Self {
        Self::body(serde_json::to_vec(value).expect("fixture json"))
    }
    fn redirect(location: String) -> Self {
        Self {
            status: 302,
            body: Vec::new(),
            location: Some(location),
        }
    }
}

fn spawn(
    listener: TcpListener,
    stop: Arc<AtomicBool>,
    mut handle: impl FnMut(Request) -> Response + Send + 'static,
) -> JoinHandle<()> {
    thread::spawn(move || {
        while !stop.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false).expect("fixture blocking");
                    stream
                        .set_read_timeout(Some(Duration::from_secs(5)))
                        .expect("fixture timeout");
                    if let Some(request) = read(&mut stream)
                        && write(&mut stream, handle(request)).is_err()
                    {
                        thread::yield_now();
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(2));
                }
                Err(_) => break,
            }
        }
    })
}

fn read(stream: &mut TcpStream) -> Option<Request> {
    let mut data = Vec::new();
    let mut chunk = [0_u8; 8192];
    let started = Instant::now();
    loop {
        if started.elapsed() > Duration::from_secs(5) || data.len() > 8 * 1024 * 1024 {
            return None;
        }
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        data.extend_from_slice(&chunk[..n]);
        let Some(end) = data.windows(4).position(|window| window == b"\r\n\r\n") else {
            continue;
        };
        let headers = std::str::from_utf8(&data[..end]).ok()?;
        let length = headers
            .lines()
            .find_map(|line| {
                line.split_once(':')
                    .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                    .and_then(|(_, value)| value.trim().parse::<usize>().ok())
            })
            .unwrap_or(0);
        if data.len() < end + 4 + length {
            continue;
        }
        let path = headers
            .lines()
            .next()?
            .split_whitespace()
            .nth(1)?
            .to_owned();
        let authorization = headers
            .lines()
            .filter_map(|line| line.split_once(':'))
            .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
            .map(|(_, value)| value.trim());
        return Some(Request {
            path,
            body: data[end + 4..end + 4 + length].to_vec(),
            authorized: authorization == Some("Bearer test-credential"),
            has_authorization: authorization.is_some(),
        });
    }
}

fn write(stream: &mut TcpStream, response: Response) -> std::io::Result<()> {
    let reason = match response.status {
        200 => "OK",
        302 => "Found",
        _ => "Not Found",
    };
    let location = response
        .location
        .map_or_else(String::new, |value| format!("Location: {value}\r\n"));
    let headers = format!(
        "HTTP/1.1 {} {reason}\r\n{location}Content-Length: {}\r\nConnection: close\r\n\r\n",
        response.status,
        response.body.len()
    );
    stream
        .write_all(headers.as_bytes())
        .and_then(|()| stream.write_all(&response.body))
}

#[path = "crate_release_qualification_native_state.rs"]
mod state;
use state::State;
