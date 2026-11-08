// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::TestResult;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use velnor_runner_host::{DaemonSelection, ImageObservation, ImageReference};
use velnor_runner_image_manifest::ImageKind;

pub(super) const ROOT: &str = "/var/lib/docker";
pub(super) const ENGINE_ID: &str = "engine-peer-a";
pub(super) const RUNNER_DIGEST: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub(super) const MISSING_DIGEST: &str =
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
pub(super) const RUNNER_ID: &str =
    "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
pub(super) const PULLED_ID: &str =
    "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

#[derive(Clone)]
pub(super) struct Response {
    status: u16,
    body: Vec<u8>,
    content_length: usize,
    close: bool,
}

impl Response {
    fn json(status: u16, value: &Value) -> Self {
        let body = value.to_string().into_bytes();
        Self {
            status,
            content_length: body.len(),
            body,
            close: false,
        }
    }

    pub(super) fn bytes(status: u16, body: Vec<u8>) -> Self {
        Self {
            status,
            content_length: body.len(),
            body,
            close: false,
        }
    }

    pub(super) fn oversized() -> Self {
        Self {
            status: 200,
            body: Vec::new(),
            content_length: 4 * 1024 * 1024 + 1,
            close: true,
        }
    }

    pub(super) fn padded_info(total_size: usize) -> Self {
        let mut response = info_response(ENGINE_ID);
        response.body.resize(total_size, b' ');
        response.content_length = total_size;
        response
    }

    pub(super) fn close(mut self) -> Self {
        self.close = true;
        self
    }
}

#[derive(Clone, Debug)]
pub(super) struct Request {
    pub(super) method: String,
    pub(super) path: String,
    pub(super) headers: Vec<(String, String)>,
    pub(super) body: Vec<u8>,
}

#[derive(Default)]
pub(super) struct Plan {
    pub(super) info: VecDeque<Response>,
    pub(super) inspect: VecDeque<Response>,
    pub(super) pull: VecDeque<Response>,
}

impl Plan {
    fn respond(&mut self, request: &Request) -> Response {
        if request.path.ends_with("/info") {
            self.info
                .pop_front()
                .unwrap_or_else(|| info_response(ENGINE_ID))
        } else if request.method == "POST" && request.path.contains("/images/create?") {
            self.pull
                .pop_front()
                .unwrap_or_else(|| Response::json(500, &json!({"message":"unexpected pull"})))
        } else if request.method == "GET" && request.path.contains("/images/") {
            self.inspect
                .pop_front()
                .unwrap_or_else(|| Response::json(500, &json!({"message":"unexpected inspection"})))
        } else {
            Response::json(500, &json!({"message":"unexpected request"}))
        }
    }
}

pub(super) struct Peer {
    path: PathBuf,
    stopping: Arc<AtomicBool>,
    requests: Arc<Mutex<Vec<Request>>>,
    active_stream: Arc<Mutex<Option<UnixStream>>>,
    handle: Option<JoinHandle<()>>,
}

impl Peer {
    pub(super) fn bind(path: &Path, mut plan: Plan) -> TestResult<Self> {
        let listener = UnixListener::bind(path)?;
        let stopping = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let active_stream = Arc::new(Mutex::new(None));
        let thread_stopping = Arc::clone(&stopping);
        let thread_requests = Arc::clone(&requests);
        let thread_active_stream = Arc::clone(&active_stream);
        let handle = thread::spawn(move || {
            loop {
                let Ok((stream, _)) = listener.accept() else {
                    break;
                };
                if thread_stopping.load(Ordering::Acquire) {
                    break;
                }
                let Ok(active) = stream.try_clone() else {
                    break;
                };
                let Ok(mut active_stream) = thread_active_stream.lock() else {
                    break;
                };
                *active_stream = Some(active);
                drop(active_stream);
                if let Err(error) =
                    serve_connection(&stream, &thread_stopping, &thread_requests, &mut plan)
                    && error.kind() != std::io::ErrorKind::UnexpectedEof
                    && error.kind() != std::io::ErrorKind::ConnectionReset
                {
                    break;
                }
                let Ok(mut active_stream) = thread_active_stream.lock() else {
                    break;
                };
                *active_stream = None;
            }
        });
        Ok(Self {
            path: path.to_owned(),
            stopping,
            requests,
            active_stream,
            handle: Some(handle),
        })
    }

    pub(super) fn requests(&self) -> TestResult<Vec<Request>> {
        Ok(self
            .requests
            .lock()
            .map_err(|_| std::io::Error::other("request log lock poisoned"))?
            .clone())
    }

    pub(super) fn moved_to(&mut self, path: &Path) {
        path.clone_into(&mut self.path);
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        if let Ok(active_stream) = self.active_stream.lock()
            && let Some(stream) = active_stream.as_ref()
        {
            drop(stream.shutdown(std::net::Shutdown::Both));
        }
        drop(UnixStream::connect(&self.path));
        if let Some(handle) = self.handle.take() {
            drop(handle.join());
        }
    }
}

fn serve_connection(
    stream: &UnixStream,
    stopping: &AtomicBool,
    requests: &Mutex<Vec<Request>>,
    plan: &mut Plan,
) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    loop {
        if stopping.load(Ordering::Acquire) {
            return Ok(());
        }
        let Some(request) = read_request(&mut reader)? else {
            return Ok(());
        };
        let response = plan.respond(&request);
        requests
            .lock()
            .map_err(|_| std::io::Error::other("request log poisoned"))?
            .push(request);
        let close = response.close;
        write_response(stream, &response)?;
        if close {
            return Ok(());
        }
    }
}

fn read_request(reader: &mut BufReader<UnixStream>) -> std::io::Result<Option<Request>> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let path = parts.next().unwrap_or_default().to_owned();
    let mut headers = Vec::new();
    let mut content_length = 0_usize;
    loop {
        line.clear();
        reader.read_line(&mut line)?;
        if line == "\r\n" || line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim().to_owned();
            if name == "content-length" {
                content_length = value.parse().map_err(std::io::Error::other)?;
            }
            headers.push((name, value));
        }
    }
    if content_length > 1024 * 1024 {
        return Err(std::io::Error::other("fixture request too large"));
    }
    let mut body = vec![0; content_length];
    reader.read_exact(&mut body)?;
    Ok(Some(Request {
        method,
        path,
        headers,
        body,
    }))
}

fn write_response(stream: &UnixStream, response: &Response) -> std::io::Result<()> {
    let reason = match response.status {
        200 => "OK",
        404 => "Not Found",
        _ => "Error",
    };
    let connection = if response.close {
        "close"
    } else {
        "keep-alive"
    };
    let mut writer = stream;
    write!(
        writer,
        "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: {}\r\n\r\n",
        response.status, reason, response.content_length, connection
    )?;
    writer.write_all(&response.body)?;
    writer.flush()?;
    Ok(())
}

pub(super) fn info_response(engine_id: &str) -> Response {
    info_response_for(engine_id, ROOT, "x86_64")
}

pub(super) fn info_response_for(engine_id: &str, root: &str, architecture: &str) -> Response {
    Response::json(
        200,
        &json!({
            "ID": engine_id,
            "DockerRootDir": root,
            "OSType": "linux",
            "Architecture": architecture
        }),
    )
}

pub(super) fn inspect_response(id: &str, digest: &str) -> Response {
    Response::json(
        200,
        &json!({
            "Id": id,
            "RepoDigests": [format!("{}@{}", ImageKind::Runner.repository(), digest)],
            "Os": "linux",
            "Architecture": "amd64",
            "RootFS": {"Type":"layers","Layers":[]}
        }),
    )
}

pub(super) fn selection() -> TestResult<DaemonSelection> {
    let mut hasher = Sha256::new();
    hasher.update(b"velnor-resource-probe-root-v1\0");
    hasher.update(ROOT.as_bytes());
    let root_digest = Sha256::digest(hasher.finalize());
    let mut digest = String::with_capacity(64);
    for byte in root_digest {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        digest.push(HEX[(byte >> 4) as usize] as char);
        digest.push(HEX[(byte & 0x0f) as usize] as char);
    }
    Ok(DaemonSelection::new(ENGINE_ID, &digest, "x86_64")?)
}

pub(super) fn image(digest: &str) -> TestResult<ImageReference> {
    Ok(ImageReference::new(ImageKind::Runner, digest)?)
}

pub(super) fn observation_matches(observation: &ImageObservation, id: &str, digest: &str) {
    assert_eq!(observation.runtime_id(), Some(id));
    assert!(
        observation
            .repository_digests()
            .iter()
            .any(|value| value == &format!("{}@{}", ImageKind::Runner.repository(), digest))
    );
    assert_eq!(observation.os(), Some("linux"));
    assert_eq!(observation.architecture(), Some("amd64"));
}

pub(super) fn pull_response(digest: &str) -> Response {
    let reference = format!("{}@{}", ImageKind::Runner.repository(), digest);
    Response::bytes(
        200,
        format!(
            "{{\"status\":\"Pulling from tailrocks/velnor-runner\",\"id\":\"runner\"}}\n{{\"status\":\"Digest: {digest}\"}}\n{{\"status\":\"Status: Image is up to date for {reference}\"}}\n"
        )
        .into_bytes(),
    )
}
