// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::BTreeMap,
    io::{self, BufReader},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use sha2::{Digest, Sha256};

#[path = "faults.rs"]
mod faults;

#[path = "http.rs"]
mod http;

use faults::{reject_tls_probe, should_drop_manifest_lookup, unauthorized_manifest};
use http::{
    Request, Response, invalid_request, json_response, not_found, read_request, response,
    write_response,
};

const REPOSITORY_PREFIX: &str = "/v2/fixture/image/";

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum ManifestFault {
    #[default]
    None,
    Unauthorized,
    Transport,
}

pub(super) struct Registry {
    address: SocketAddr,
    state: Arc<Mutex<State>>,
    stopped: Arc<AtomicBool>,
    server: Option<JoinHandle<()>>,
}

#[derive(Default)]
struct State {
    blobs: BTreeMap<String, Vec<u8>>,
    manifests: BTreeMap<String, Vec<u8>>,
    tags: BTreeMap<String, String>,
    uploads: BTreeMap<String, Vec<u8>>,
    next_upload: u64,
    failure: Option<String>,
    manifest_fault: ManifestFault,
}

impl Registry {
    pub(super) fn new() -> io::Result<Self> {
        Self::with_manifest_fault(ManifestFault::None)
    }

    pub(super) fn with_manifest_fault(manifest_fault: ManifestFault) -> io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;
        let state = Arc::new(Mutex::new(State {
            manifest_fault,
            ..State::default()
        }));
        let stopped = Arc::new(AtomicBool::new(false));
        let server = thread::spawn({
            let state = Arc::clone(&state);
            let stopped = Arc::clone(&stopped);
            move || serve(&listener, &state, &stopped)
        });
        Ok(Self {
            address,
            state,
            stopped,
            server: Some(server),
        })
    }

    pub(super) fn reference(&self, tag: &str) -> String {
        format!("{}/fixture/image:{tag}", self.address)
    }

    pub(super) fn digest_reference(&self, digest: &str) -> String {
        format!("{}/fixture/image@{digest}", self.address)
    }

    pub(super) fn stored_inventory(&self) -> io::Result<BTreeMap<String, Vec<u8>>> {
        let state = self.state.lock().map_err(lock_error)?;
        Ok(state
            .blobs
            .iter()
            .map(|(digest, bytes)| {
                (
                    format!("blobs/sha256/{}", digest.trim_start_matches("sha256:")),
                    bytes.clone(),
                )
            })
            .collect())
    }

    pub(super) fn manifest_bytes(&self, digest: &str) -> io::Result<Option<Vec<u8>>> {
        let state = self.state.lock().map_err(lock_error)?;
        Ok(state.manifests.get(digest).cloned())
    }

    pub(super) fn corrupt_blob(&self, digest: &str) -> io::Result<()> {
        let mut state = self.state.lock().map_err(lock_error)?;
        let bytes = state.blobs.get_mut(digest).ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "remote fixture blob is missing")
        })?;
        let middle = bytes.len() / 2;
        let byte = bytes
            .get_mut(middle)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "remote blob is empty"))?;
        *byte ^= 0xff;
        Ok(())
    }

    pub(super) fn require_healthy(&self) -> io::Result<()> {
        let state = self.state.lock().map_err(lock_error)?;
        match &state.failure {
            Some(error) => Err(io::Error::other(error.clone())),
            None => Ok(()),
        }
    }
}

impl Drop for Registry {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        if let Some(server) = self.server.take()
            && let Err(error) = server.join()
        {
            eprintln!("registry fixture server thread failed: {error:?}");
        }
    }
}

fn serve(listener: &TcpListener, state: &Mutex<State>, stopped: &AtomicBool) {
    while !stopped.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, _)) => {
                if let Err(error) = serve_connection(stream, state) {
                    record_failure(state, error.to_string());
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(2));
            }
            Err(error) => record_failure(state, error.to_string()),
        }
    }
}

fn serve_connection(stream: TcpStream, state: &Mutex<State>) -> io::Result<()> {
    let mut stream = stream;
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    if reject_tls_probe(&mut stream)? {
        return Ok(());
    }
    let mut reader = BufReader::new(stream);
    let request = read_request(&mut reader)?;
    if should_drop_manifest_lookup(&request, state)? {
        return Ok(());
    }
    let head = request.method == "HEAD";
    let result = route(&request, state);
    let response = match result {
        Ok(response) => response,
        Err(error) => {
            record_failure(state, error.to_string());
            json_response(
                500,
                "Internal Server Error",
                b"registry fixture error".to_vec(),
            )
        }
    };
    write_response(reader.get_mut(), response, head)
}

fn route(request: &Request, state: &Mutex<State>) -> io::Result<Response> {
    if request.path == "/v2/" {
        return Ok(api_response());
    }
    let path = request
        .path
        .strip_prefix(REPOSITORY_PREFIX)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "unknown registry route"))?;
    let mut state = state.lock().map_err(lock_error)?;
    if path == "blobs/uploads" || path == "blobs/uploads/" {
        return Ok(start_upload(request, &mut state));
    }
    if let Some(upload) = path.strip_prefix("blobs/uploads/") {
        return upload_blob(request, &mut state, upload);
    }
    if let Some(digest) = path.strip_prefix("blobs/") {
        return Ok(serve_blob(request, &state, digest));
    }
    if let Some(reference) = path.strip_prefix("manifests/") {
        return Ok(serve_manifest(request, &mut state, reference));
    }
    Ok(not_found("UNSUPPORTED", "unsupported registry route"))
}

fn api_response() -> Response {
    response(200, "OK", Vec::new(), Vec::new())
}

fn start_upload(request: &Request, state: &mut State) -> Response {
    if request.method != "POST" {
        return not_found("UNSUPPORTED", "upload method is unsupported");
    }
    if let Some(digest) = query_value(request.query.as_deref(), "mount")
        && state.blobs.contains_key(&digest)
    {
        return upload_complete(&digest);
    }
    state.next_upload = state.next_upload.saturating_add(1);
    let upload = format!("upload-{}", state.next_upload);
    state.uploads.insert(upload.clone(), request.body.clone());
    upload_started(&upload)
}

fn upload_blob(request: &Request, state: &mut State, upload: &str) -> io::Result<Response> {
    match request.method.as_str() {
        "PATCH" => {
            let bytes = state.uploads.get_mut(upload).ok_or_else(invalid_request)?;
            bytes.extend_from_slice(&request.body);
            Ok(upload_progress(upload))
        }
        "PUT" => {
            let digest =
                query_value(request.query.as_deref(), "digest").ok_or_else(invalid_request)?;
            let mut bytes = state.uploads.remove(upload).ok_or_else(invalid_request)?;
            bytes.extend_from_slice(&request.body);
            if digest_bytes(&bytes) != digest {
                return Ok(not_found("DIGEST_INVALID", "uploaded digest did not match"));
            }
            state.blobs.insert(digest.clone(), bytes);
            Ok(upload_complete(&digest))
        }
        _ => Ok(not_found("UNSUPPORTED", "upload method is unsupported")),
    }
}

fn serve_blob(request: &Request, state: &State, digest: &str) -> Response {
    if !matches!(request.method.as_str(), "GET" | "HEAD") {
        return not_found("UNSUPPORTED", "blob method is unsupported");
    }
    match state.blobs.get(digest) {
        Some(bytes) => blob_response(digest, bytes.clone()),
        None => not_found("BLOB_UNKNOWN", "blob unknown"),
    }
}

fn serve_manifest(request: &Request, state: &mut State, reference: &str) -> Response {
    if !matches!(request.method.as_str(), "GET" | "HEAD" | "PUT") {
        return not_found("UNSUPPORTED", "manifest method is unsupported");
    }
    if request.method == "PUT" {
        let digest = digest_bytes(&request.body);
        state.manifests.insert(digest.clone(), request.body.clone());
        state.tags.insert(reference.to_owned(), digest.clone());
        return manifest_written(&digest);
    }
    if state.manifest_fault == ManifestFault::Unauthorized {
        return unauthorized_manifest();
    }
    let digest = state.tags.get(reference).map_or(reference, String::as_str);
    match state.manifests.get(digest) {
        Some(bytes) => manifest_response(digest, bytes.clone()),
        None => not_found("MANIFEST_UNKNOWN", "manifest unknown"),
    }
}

fn upload_started(upload: &str) -> Response {
    response(
        202,
        "Accepted",
        vec![
            ("Location".to_owned(), upload_location(upload)),
            ("Docker-Upload-UUID".to_owned(), upload.to_owned()),
        ],
        Vec::new(),
    )
}

fn upload_progress(upload: &str) -> Response {
    upload_started(upload)
}

fn upload_complete(digest: &str) -> Response {
    response(
        201,
        "Created",
        vec![
            (
                "Location".to_owned(),
                format!("{REPOSITORY_PREFIX}blobs/{digest}"),
            ),
            ("Docker-Content-Digest".to_owned(), digest.to_owned()),
        ],
        Vec::new(),
    )
}

fn upload_location(upload: &str) -> String {
    format!("{REPOSITORY_PREFIX}blobs/uploads/{upload}")
}

fn blob_response(digest: &str, bytes: Vec<u8>) -> Response {
    response(
        200,
        "OK",
        vec![
            ("Docker-Content-Digest".to_owned(), digest.to_owned()),
            (
                "Content-Type".to_owned(),
                "application/octet-stream".to_owned(),
            ),
        ],
        bytes,
    )
}

fn manifest_written(digest: &str) -> Response {
    response(
        201,
        "Created",
        vec![("Docker-Content-Digest".to_owned(), digest.to_owned())],
        Vec::new(),
    )
}

fn manifest_response(digest: &str, bytes: Vec<u8>) -> Response {
    response(
        200,
        "OK",
        vec![
            ("Docker-Content-Digest".to_owned(), digest.to_owned()),
            (
                "Content-Type".to_owned(),
                "application/vnd.oci.image.manifest.v1+json".to_owned(),
            ),
        ],
        bytes,
    )
}

fn query_value(query: Option<&str>, key: &str) -> Option<String> {
    query?
        .split('&')
        .filter_map(|part| part.split_once('='))
        .find(|(name, _)| *name == key)
        .map(|(_, value)| value.replace("%3A", ":").replace("%3a", ":"))
}

fn digest_bytes(bytes: &[u8]) -> String {
    format!("sha256:{}", hex(&Sha256::digest(bytes)))
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|byte| {
            [
                char::from(HEX[usize::from(byte >> 4)]),
                char::from(HEX[usize::from(byte & 15)]),
            ]
        })
        .collect()
}

fn record_failure(state: &Mutex<State>, error: String) {
    if let Ok(mut state) = state.lock() {
        state.failure.get_or_insert(error);
    }
}

fn lock_error<T>(_: std::sync::PoisonError<T>) -> io::Error {
    io::Error::other("registry fixture state is poisoned")
}
