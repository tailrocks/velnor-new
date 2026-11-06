// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "gateway/http.rs"]
mod http;
#[path = "gateway/policy.rs"]
mod policy;
#[path = "gateway/transport.rs"]
mod transport;
pub use transport::{RegistryIo, RegistryRequest, RegistryResponse};

use crate::{OrchestratorError, internal::internal};
use base64::{Engine, engine::general_purpose::STANDARD};
use std::{
    fs,
    io::{self, Read, Write},
    net::{TcpListener, TcpStream},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use tempfile::TempDir;
use velnor_actions_mise::CheckDeadline;

#[derive(Debug)]
pub struct RegistryGateway {
    address: String,
    config: TempDir,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl RegistryGateway {
    pub(super) fn start(
        config: &Path,
        repository: &str,
        deadline: CheckDeadline,
    ) -> Result<Self, OrchestratorError> {
        let mut bytes = Vec::new();
        fs::File::open(config.join("config.json"))
            .map_err(|_| internal("image_publication_registry_credentials"))?
            .take(65_537)
            .read_to_end(&mut bytes)
            .map_err(|_| internal("image_publication_registry_credentials"))?;
        if bytes.len() > 65_536 {
            return Err(internal("image_publication_registry_credentials"));
        }
        let value: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|_| internal("image_publication_registry_credentials"))?;
        let authorization = value
            .get("auths")
            .and_then(|v| v.get("ghcr.io"))
            .and_then(|v| v.get("auth"))
            .and_then(serde_json::Value::as_str)
            .filter(|auth| !auth.is_empty() && auth.len() <= 16_384)
            .ok_or_else(|| internal("image_publication_registry_credentials"))?;
        Self::with_io(
            repository,
            authorization.to_owned(),
            Arc::new(transport::UreqIo::new(deadline)),
            deadline,
        )
    }

    pub fn with_io(
        repository: &str,
        authorization: String,
        io: Arc<dyn RegistryIo>,
        deadline: CheckDeadline,
    ) -> Result<Self, OrchestratorError> {
        if authorization.is_empty() || authorization.len() > 16_384 {
            return Err(internal("image_publication_registry_credentials"));
        }
        let decoded = STANDARD
            .decode(&authorization)
            .map_err(|_| internal("image_publication_registry_credentials"))?;
        if !decoded.contains(&b':') || decoded.iter().any(|byte| *byte < 32 || *byte == 127) {
            return Err(internal("image_publication_registry_credentials"));
        }
        if !velnor_actions_contract::config::is_valid_image_repository(repository)
            || !repository.starts_with("ghcr.io/tailrocks/")
        {
            return Err(internal("image_publication_registry_repository"));
        }
        let repository = repository
            .strip_prefix("ghcr.io/")
            .ok_or_else(|| internal("image_publication_registry_repository"))?;
        let transport = Arc::new(transport::Transport::new(
            authorization,
            repository.to_owned(),
            io,
        ));
        let config = tempfile::Builder::new()
            .prefix("velnor-crane-")
            .tempdir()
            .map_err(|_| internal("image_publication_registry_config"))?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(config.path().join("config.json"))
            .map_err(|_| internal("image_publication_registry_config"))?;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .and_then(|()| file.write_all(b"{}"))
            .map_err(|_| internal("image_publication_registry_config"))?;
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|_| internal("image_publication_registry_listener"))?;
        let address = listener
            .local_addr()
            .map_err(|_| internal("image_publication_registry_listener"))?
            .to_string();
        listener
            .set_nonblocking(true)
            .map_err(|_| internal("image_publication_registry_listener"))?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let repo = repository.to_owned();
        let local = address.clone();
        let thread =
            thread::spawn(move || serve(&listener, &transport, &repo, &local, &flag, deadline));
        Ok(Self {
            address,
            config,
            stop,
            thread: Some(thread),
        })
    }

    #[must_use]
    pub fn repository(&self, repository: &str) -> String {
        format!(
            "{}/{}",
            self.address,
            repository.strip_prefix("ghcr.io/").unwrap_or_default()
        )
    }

    #[must_use]
    pub fn config(&self) -> PathBuf {
        self.config.path().to_owned()
    }
}

impl Drop for RegistryGateway {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            drop(thread.join());
        }
    }
}

fn handle(
    stream: TcpStream,
    repository: &str,
    local: &str,
    transport: &transport::Transport,
    deadline: CheckDeadline,
) -> io::Result<()> {
    stream.set_nonblocking(false)?;
    let mut request = http::Request::read(stream, deadline, 1024 * 1024 * 1024)?;
    if request.method == "GET" && request.target == "/v2/" {
        return request.respond(
            200,
            &[("content-length".into(), "0".into())],
            &mut io::empty(),
        );
    }
    let (upload, digest) = classify(&request.method, &request.target, repository)?;
    let headers: Vec<_> = request
        .headers
        .iter()
        .filter(|(name, _)| matches!(name.as_str(), "accept" | "content-type" | "content-length"))
        .cloned()
        .collect();
    let target = request.target.clone();
    let method = request.method.clone();
    let mut response = transport
        .send(&method, &target, &headers, &mut request)
        .map_err(|_| io::ErrorKind::Other)?;
    if (300..400).contains(&response.status) {
        if method != "GET" || digest.is_none() || !matches!(response.status, 302 | 307) {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        if response
            .headers
            .iter()
            .filter(|(name, _)| name.eq_ignore_ascii_case("location"))
            .count()
            != 1
        {
            return Err(io::ErrorKind::InvalidData.into());
        }
        let location = response
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("location"))
            .and_then(|(_, value)| policy::cdn_location(value, digest.as_deref()?))
            .ok_or(io::ErrorKind::PermissionDenied)?;
        response = transport.cdn(&location).map_err(|_| io::ErrorKind::Other)?;
        if (300..400).contains(&response.status) {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
    }
    let status = response.status;
    let continuation = upload && status == 202 && matches!(method.as_str(), "POST" | "PATCH");
    let output = response_headers(&response.headers, continuation, &target, local, repository)?;
    let limit = if status >= 400 || upload {
        65_536
    } else if digest.is_some() {
        1024 * 1024 * 1024
    } else {
        super::OUTPUT_LIMIT as u64
    };
    let mut body = BoundedBody {
        inner: response.body,
        remaining: limit,
    };
    request.respond(status, &output, &mut body)
}

struct BoundedBody {
    inner: Box<dyn Read + Send>,
    remaining: u64,
}

impl Read for BoundedBody {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            let mut extra = [0];
            return if self.inner.read(&mut extra)? == 0 {
                Ok(0)
            } else {
                Err(io::ErrorKind::InvalidData.into())
            };
        }
        let limit = usize::try_from(self.remaining.min(buffer.len() as u64))
            .map_err(|_| io::ErrorKind::InvalidData)?;
        let count = self.inner.read(&mut buffer[..limit])?;
        self.remaining -= count as u64;
        Ok(count)
    }
}

fn classify(method: &str, target: &str, repository: &str) -> io::Result<(bool, Option<String>)> {
    let prefix = format!("/v2/{repository}/");
    let Some(route) = target.strip_prefix(&prefix) else {
        return Err(io::ErrorKind::PermissionDenied.into());
    };
    let (path, query) = route.split_once('?').unwrap_or((route, ""));
    let manifest = path.strip_prefix("manifests/").is_some_and(|name| {
        !name.is_empty()
            && !matches!(name, "." | "..")
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b))
            && query.is_empty()
    });
    let blob = path.strip_prefix("blobs/").filter(|name| {
        name.starts_with("sha256:")
            && name.len() == 71
            && name[7..]
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    });
    let upload_path = target.split('?').next().unwrap_or_default();
    let upload = (path.starts_with("blobs/upload/") || path.starts_with("blobs/uploads/"))
        && policy::upload_location(upload_path, upload_path, repository).is_some()
        && policy::safe_query(query);
    if !(manifest && matches!(method, "GET" | "HEAD" | "PUT")
        || blob.is_some() && query.is_empty() && matches!(method, "GET" | "HEAD")
        || upload && matches!(method, "POST" | "PATCH" | "PUT"))
    {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    Ok((upload, blob.map(str::to_owned)))
}

fn serve(
    listener: &TcpListener,
    transport: &Arc<transport::Transport>,
    repo: &str,
    local: &str,
    flag: &AtomicBool,
    deadline: CheckDeadline,
) {
    let mut workers: Vec<JoinHandle<()>> = Vec::new();
    while !flag.load(Ordering::Acquire) && deadline.remaining().is_ok() {
        match listener.accept() {
            Ok((stream, _)) => {
                let mut active = Vec::new();
                for worker in workers.drain(..) {
                    if worker.is_finished() {
                        drop(worker.join());
                    } else {
                        active.push(worker);
                    }
                }
                workers = active;
                if workers.len() >= 16 {
                    continue;
                }
                let transport = Arc::clone(transport);
                let repo = repo.to_owned();
                let local = local.to_owned();
                workers.push(thread::spawn(move || {
                    drop(handle(stream, &repo, &local, &transport, deadline));
                }));
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(_) => break,
        }
    }
    for worker in workers {
        drop(worker.join());
    }
}

fn response_headers(
    headers: &[(String, String)],
    continuation: bool,
    target: &str,
    local: &str,
    repository: &str,
) -> io::Result<Vec<(String, String)>> {
    let mut output = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for (name, value) in headers {
        let lower = name.to_ascii_lowercase();
        if matches!(
            lower.as_str(),
            "location"
                | "content-length"
                | "content-type"
                | "docker-content-digest"
                | "docker-upload-uuid"
                | "range"
        ) && !seen.insert(lower.clone())
        {
            return Err(io::ErrorKind::InvalidData.into());
        }
        if lower == "content-length"
            && (value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()))
        {
            return Err(io::ErrorKind::InvalidData.into());
        }
        if [
            "content-type",
            "content-length",
            "docker-content-digest",
            "docker-upload-uuid",
            "range",
        ]
        .iter()
        .any(|allowed| name.eq_ignore_ascii_case(allowed))
        {
            output.push((name.to_owned(), value.to_owned()));
        }
        if name.eq_ignore_ascii_case("location") && continuation {
            let location = policy::upload_location(target, value, repository)
                .ok_or(io::ErrorKind::PermissionDenied)?;
            output.push(("location".into(), format!("http://{local}{location}")));
        }
    }
    Ok(output)
}
