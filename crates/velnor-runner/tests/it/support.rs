// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use anyhow::{Context, Error, Result, anyhow};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use velnor_runner::protocol::GitHubApiError;

mod jit;
mod mock_broker;
pub(crate) mod mock_feed;
pub(crate) mod mock_github;
mod mock_results;

pub(crate) const TOKEN: &str = "test-token";

const MAX_REQUEST_BYTES: usize = 1 << 20;

#[derive(Debug, Clone)]
pub(crate) struct RecordedRequest {
    pub(crate) method: String,
    pub(crate) path: String,
    query: Vec<(String, String)>,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    pub(crate) at: Instant,
}

impl RecordedRequest {
    pub(crate) fn query(&self, name: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    pub(crate) fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    pub(crate) fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub(crate) fn json(&self) -> Result<Value> {
        serde_json::from_slice(&self.body)
            .with_context(|| format!("{} {} body is JSON", self.method, self.path))
    }
}

#[derive(Clone)]
pub(crate) struct Reply {
    status: u16,
    content_type: &'static str,
    body: String,
    stall: bool,
    delay: Duration,
    header: Option<(&'static str, &'static str)>,
}

impl Reply {
    pub(crate) fn status(status: u16) -> Self {
        Self::text(status, "")
    }

    pub(crate) fn text(status: u16, body: &str) -> Self {
        Self {
            status,
            content_type: "text/plain",
            body: body.to_owned(),
            stall: false,
            delay: Duration::ZERO,
            header: None,
        }
    }

    pub(crate) fn stall() -> Self {
        Self {
            stall: true,
            ..Self::status(200)
        }
    }

    pub(crate) fn after(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    pub(crate) fn with_header(mut self, name: &'static str, value: &'static str) -> Self {
        self.header = Some((name, value));
        self
    }

    pub(crate) fn json(status: u16, body: &Value) -> Self {
        Self {
            status,
            content_type: "application/json",
            body: body.to_string(),
            stall: false,
            delay: Duration::ZERO,
            header: None,
        }
    }
}

type Responder = Box<dyn Fn(&RecordedRequest) -> Reply + Send + Sync>;

struct Route {
    method: &'static str,
    path: String,
    responder: Responder,
}

#[derive(Default)]
struct State {
    routes: Vec<Route>,
    requests: Vec<RecordedRequest>,
    in_flight: BTreeMap<String, (usize, usize)>,
}

pub(crate) struct TestServer {
    address: SocketAddr,
    state: Arc<Mutex<State>>,
    accept_loop: JoinHandle<()>,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.accept_loop.abort();
    }
}

impl TestServer {
    pub(crate) async fn start() -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .context("bind test server")?;
        let address = listener.local_addr().context("test server address")?;
        let state = Arc::new(Mutex::new(State::default()));
        let accept_state = Arc::clone(&state);
        let accept_loop = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(serve_connection(stream, Arc::clone(&accept_state)));
            }
        });
        Ok(Self {
            address,
            state,
            accept_loop,
        })
    }

    pub(crate) fn uri(&self) -> String {
        format!("http://{}", self.address)
    }

    pub(crate) fn respond(
        &self,
        method: &'static str,
        path: &str,
        responder: impl Fn(&RecordedRequest) -> Reply + Send + Sync + 'static,
    ) -> Result<()> {
        lock(&self.state)?.routes.push(Route {
            method,
            path: path.to_owned(),
            responder: Box::new(responder),
        });
        Ok(())
    }

    pub(crate) fn reset(&self) -> Result<()> {
        let mut state = lock(&self.state)?;
        state.routes.clear();
        state.requests.clear();
        Ok(())
    }

    pub(crate) fn requests(&self) -> Result<Vec<RecordedRequest>> {
        Ok(lock(&self.state)?.requests.clone())
    }

    pub(crate) fn requests_to(&self, method: &str, path: &str) -> Result<Vec<RecordedRequest>> {
        Ok(self
            .requests()?
            .into_iter()
            .filter(|request| request.method == method && request.path == path)
            .collect())
    }

    pub(crate) fn peak_concurrency(&self, path: &str) -> Result<usize> {
        Ok(lock(&self.state)?
            .in_flight
            .get(path)
            .map_or(0, |(_, peak)| *peak))
    }

    pub(crate) fn only_request(&self, method: &str, path: &str) -> Result<RecordedRequest> {
        let mut requests = self.requests_to(method, path)?;
        match requests.len() {
            1 => requests.pop().context("request vanished"),
            count => Err(anyhow!("expected one {method} {path}, saw {count}")),
        }
    }
}

fn lock(state: &Mutex<State>) -> Result<MutexGuard<'_, State>> {
    state
        .lock()
        .map_err(|_| anyhow!("test server state poisoned"))
}

async fn serve_connection(mut stream: TcpStream, state: Arc<Mutex<State>>) {
    let Ok(request) = read_request(&mut stream).await else {
        return;
    };
    let reply = {
        let Ok(mut state) = lock(&state) else {
            return;
        };
        state.requests.push(request.clone());
        let (current, peak) = state.in_flight.entry(request.path.clone()).or_default();
        *current += 1;
        *peak = (*peak).max(*current);
        match state
            .routes
            .iter()
            .find(|route| route.method == request.method && route.path == request.path)
        {
            Some(route) => (route.responder)(&request),
            None => Reply::text(404, "no route"),
        }
    };
    if reply.stall {
        std::future::pending::<()>().await;
    }
    tokio::time::sleep(reply.delay).await;
    if let Ok(mut state) = lock(&state)
        && let Some((current, _)) = state.in_flight.get_mut(&request.path)
    {
        *current -= 1;
    }
    let extra = reply
        .header
        .map_or_else(String::new, |(name, value)| format!("{name}: {value}\r\n"));
    let head = format!(
        "HTTP/1.1 {} Test\r\ncontent-type: {}\r\ncontent-length: {}\r\n{extra}connection: close\r\n\r\n",
        reply.status,
        reply.content_type,
        reply.body.len()
    );
    let _ = stream.write_all(head.as_bytes()).await;
    let _ = stream.write_all(reply.body.as_bytes()).await;
    let _ = stream.shutdown().await;
}

async fn read_request(stream: &mut TcpStream) -> Result<RecordedRequest> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    let head_end = loop {
        if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break position;
        }
        if buffer.len() > MAX_REQUEST_BYTES {
            return Err(anyhow!("request head too large"));
        }
        let read = stream.read(&mut chunk).await.context("read request")?;
        if read == 0 {
            return Err(anyhow!("connection closed before request head"));
        }
        buffer.extend_from_slice(&chunk[..read]);
    };
    let head = String::from_utf8(buffer[..head_end].to_vec()).context("request head is UTF-8")?;
    let mut lines = head.split("\r\n");
    let request_line = lines.next().context("request line")?;
    let mut parts = request_line.split(' ');
    let method = parts.next().context("request method")?.to_owned();
    let target = parts.next().context("request target")?;
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_owned(), value.trim().to_owned()))
        .collect();
    let content_length = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .map(|(_, value)| value.parse::<usize>())
        .transpose()
        .context("content-length")?
        .unwrap_or(0);
    let mut body = buffer[head_end + 4..].to_vec();
    while body.len() < content_length {
        let read = stream.read(&mut chunk).await.context("read request body")?;
        if read == 0 {
            return Err(anyhow!("connection closed before request body"));
        }
        body.extend_from_slice(&chunk[..read]);
    }
    Ok(RecordedRequest {
        method,
        path: path.to_owned(),
        query: url::form_urlencoded::parse(query.as_bytes())
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect(),
        headers,
        body,
        at: Instant::now(),
    })
}

pub(crate) fn assert_bearer(request: &RecordedRequest) {
    assert_eq!(
        request.header("authorization"),
        Some(format!("Bearer {TOKEN}").as_str())
    );
}

pub(crate) fn assert_github_status(error: &Error, status: u16, action: &str) -> Result<()> {
    let api_error = error
        .downcast_ref::<GitHubApiError>()
        .with_context(|| format!("expected GitHubApiError, got {error:#}"))?;
    assert_eq!(api_error.status, status);
    assert_eq!(api_error.action, action);
    Ok(())
}
