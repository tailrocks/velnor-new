// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{OrchestratorError, internal::internal};
use std::{io::Read, path::Path, time::Instant};

pub struct ToolDownloadResponse {
    pub status: u16,
    pub location: Option<String>,
    pub content_length: Option<u64>,
    pub content_encoding: Option<String>,
    pub body: Box<dyn Read>,
}

pub trait ToolDownloadTransport {
    fn github(
        &self,
        repository: &str,
        id: u64,
        token: &str,
        deadline: Instant,
    ) -> Result<ToolDownloadResponse, OrchestratorError>;
    fn storage(
        &self,
        url: &str,
        deadline: Instant,
    ) -> Result<ToolDownloadResponse, OrchestratorError>;
}

pub struct ToolDownloadRequest<'a> {
    pub repository: &'a str,
    pub id: u64,
    pub token: &'a str,
    pub expected_size: u64,
    pub expected_sha256: &'a str,
    pub destination: &'a Path,
    pub deadline: Instant,
}

pub fn download_tool_artifact<T: ToolDownloadTransport>(
    transport: &T,
    request: &ToolDownloadRequest<'_>,
) -> Result<std::fs::File, OrchestratorError> {
    let invalid = || internal("crate_release_tool_download_invalid");
    let Some((owner, repository)) = request.repository.split_once('/') else {
        return Err(invalid());
    };
    if !velnor_actions_contract::config::is_valid_github_owner(owner)
        || !velnor_actions_contract::config::is_valid_github_repo_name(repository)
        || request.id == 0
        || request.token.is_empty()
        || request.token.bytes().any(|byte| byte.is_ascii_control())
        || request.expected_size == 0
        || request.expected_size > 1024 * 1024 * 1024
        || request.expected_sha256.len() != 64
        || !request
            .expected_sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || !request.destination.is_absolute()
        || Instant::now() >= request.deadline
    {
        return Err(invalid());
    }
    let parent = open_download_parent(request.destination.parent().ok_or_else(invalid)?)?;
    let name = request.destination.file_name().ok_or_else(invalid)?;
    let github = transport.github(
        request.repository,
        request.id,
        request.token,
        request.deadline,
    )?;
    if github.status != 302 {
        return Err(invalid());
    }
    let mut location = validate_storage_url(github.location.as_deref().ok_or_else(invalid)?)?;
    for redirects in 0..=3 {
        if Instant::now() >= request.deadline {
            return Err(invalid());
        }
        let response = transport.storage(&location, request.deadline)?;
        if matches!(response.status, 301 | 302 | 303 | 307 | 308) {
            if redirects == 3 {
                return Err(invalid());
            }
            location = validate_storage_url(response.location.as_deref().ok_or_else(invalid)?)?;
            continue;
        }
        if response.status != 200
            || response
                .content_length
                .is_some_and(|length| length != request.expected_size)
            || response
                .content_encoding
                .as_deref()
                .is_some_and(|value| !value.eq_ignore_ascii_case("identity"))
        {
            return Err(invalid());
        }
        return stream_download(response.body, request, &parent, name);
    }
    Err(invalid())
}

fn validate_storage_url(value: &str) -> Result<String, OrchestratorError> {
    let invalid = || internal("crate_release_tool_download_redirect");
    let uri = value.parse::<ureq::http::Uri>().map_err(|_| invalid())?;
    let authority = uri.authority().ok_or_else(invalid)?;
    let host = authority.host().to_ascii_lowercase();
    if uri.scheme_str() != Some("https")
        || value.contains('#')
        || (!authority.as_str().eq_ignore_ascii_case(&host)
            && !authority
                .as_str()
                .eq_ignore_ascii_case(&format!("{host}:443")))
        || !(host.ends_with(".blob.core.windows.net")
            || host.ends_with(".actions.githubusercontent.com"))
    {
        return Err(invalid());
    }
    Ok(uri.to_string())
}

#[cfg(unix)]
pub(crate) fn open_download_parent(
    parent: &Path,
) -> Result<rustix::fd::OwnedFd, OrchestratorError> {
    use rustix::fs::{Mode, OFlags, open, openat};
    let invalid = || internal("crate_release_tool_download_destination");
    if !parent.is_absolute() {
        return Err(invalid());
    }
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut current = open("/", flags, Mode::empty()).map_err(|_| invalid())?;
    for component in parent.components() {
        match component {
            std::path::Component::RootDir => {}
            std::path::Component::Normal(name) => {
                current = openat(&current, name, flags, Mode::empty()).map_err(|_| invalid())?;
            }
            _ => return Err(invalid()),
        }
    }
    Ok(current)
}

#[cfg(unix)]
fn stream_download(
    mut body: Box<dyn Read>,
    request: &ToolDownloadRequest<'_>,
    parent: &rustix::fd::OwnedFd,
    name: &std::ffi::OsStr,
) -> Result<std::fs::File, OrchestratorError> {
    use sha2::{Digest, Sha256};
    use std::io::{Seek, Write};
    let invalid = || internal("crate_release_tool_download_invalid");
    let mut file = anonymous_download_file(parent, name)?;
    let mut hash = Sha256::new();
    let mut size = 0u64;
    let mut buffer = [0u8; 16 * 1024];
    loop {
        if Instant::now() >= request.deadline {
            return Err(invalid());
        }
        let count = body.read(&mut buffer).map_err(|_| invalid())?;
        if count == 0 {
            break;
        }
        size = size
            .checked_add(count as u64)
            .filter(|size| *size <= request.expected_size)
            .ok_or_else(invalid)?;
        hash.update(&buffer[..count]);
        file.write_all(&buffer[..count]).map_err(|_| invalid())?;
    }
    file.flush().map_err(|_| invalid())?;
    if size != request.expected_size
        || crate::config::playwright_lock::hex(&hash.finalize()) != request.expected_sha256
    {
        return Err(invalid());
    }
    file.rewind().map_err(|_| invalid())?;
    Ok(file)
}

#[cfg(target_os = "linux")]
fn anonymous_download_file(
    parent: &rustix::fd::OwnedFd,
    name: &std::ffi::OsStr,
) -> Result<std::fs::File, OrchestratorError> {
    use rustix::fs::{AtFlags, Mode, OFlags, openat, statat};
    let invalid = || internal("crate_release_tool_download_invalid");
    match statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) {
        Err(rustix::io::Errno::NOENT) => {}
        _ => return Err(invalid()),
    }
    let fd = openat(
        parent,
        ".",
        OFlags::RDWR | OFlags::TMPFILE | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
    )
    .map_err(|_| invalid())?;
    Ok(std::fs::File::from(fd))
}

#[cfg(all(unix, not(target_os = "linux")))]
fn anonymous_download_file(
    parent: &rustix::fd::OwnedFd,
    name: &std::ffi::OsStr,
) -> Result<std::fs::File, OrchestratorError> {
    use rustix::fs::{AtFlags, Mode, OFlags, fstat, openat, statat, unlinkat};
    let invalid = || internal("crate_release_tool_download_invalid");
    let fd = openat(
        parent,
        name,
        OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
    )
    .map_err(|_| invalid())?;
    let file = std::fs::File::from(fd);
    let opened = fstat(&file).map_err(|_| invalid())?;
    let named = statat(parent, name, AtFlags::SYMLINK_NOFOLLOW).map_err(|_| invalid())?;
    if opened.st_dev != named.st_dev || opened.st_ino != named.st_ino || named.st_nlink != 1 {
        return Err(internal("crate_release_tool_download_identity"));
    }
    unlinkat(parent, name, AtFlags::empty())
        .map_err(|_| internal("crate_release_tool_download_cleanup"))?;
    Ok(file)
}

#[cfg(not(unix))]
pub(crate) fn open_download_parent(
    _parent: &Path,
) -> Result<rustix::fd::OwnedFd, OrchestratorError> {
    Err(internal("crate_release_tool_download_platform_unsupported"))
}
#[cfg(not(unix))]
fn stream_download(
    _body: Box<dyn Read>,
    _request: &ToolDownloadRequest<'_>,
    _parent: &rustix::fd::OwnedFd,
    _name: &std::ffi::OsStr,
) -> Result<std::fs::File, OrchestratorError> {
    Err(internal("crate_release_tool_download_platform_unsupported"))
}

impl std::fmt::Debug for ToolDownloadResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolDownloadResponse")
            .field("status", &self.status)
            .field("content_length", &self.content_length)
            .finish_non_exhaustive()
    }
}
impl std::fmt::Debug for ToolDownloadRequest<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolDownloadRequest")
            .field("repository", &self.repository)
            .field("id", &self.id)
            .field("expected_size", &self.expected_size)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Default)]
pub struct NativeToolDownloadTransport;

impl ToolDownloadTransport for NativeToolDownloadTransport {
    fn github(
        &self,
        repository: &str,
        id: u64,
        token: &str,
        deadline: Instant,
    ) -> Result<ToolDownloadResponse, OrchestratorError> {
        let Some((owner, name)) = repository.split_once('/') else {
            return Err(internal("crate_release_tool_download_invalid"));
        };
        if !velnor_actions_contract::config::is_valid_github_owner(owner)
            || !velnor_actions_contract::config::is_valid_github_repo_name(name)
            || id == 0
            || token.is_empty()
            || token.bytes().any(|byte| byte.is_ascii_control())
        {
            return Err(internal("crate_release_tool_download_invalid"));
        }
        let url = format!("https://api.github.com/repos/{repository}/actions/artifacts/{id}/zip");
        let request = crate::api::github_evidence_http::agent()?
            .get(url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("Accept-Encoding", "identity")
            .header("Cache-Control", "no-cache, no-store")
            .header("User-Agent", "velnor-actions-tool-artifact");
        native_download_request(request, deadline)
    }
    fn storage(
        &self,
        url: &str,
        deadline: Instant,
    ) -> Result<ToolDownloadResponse, OrchestratorError> {
        let safe_url = validate_storage_url(url)?;
        let request = crate::api::github_evidence_http::agent()?
            .get(safe_url)
            .header("Accept-Encoding", "identity")
            .header("User-Agent", "velnor-actions-tool-artifact");
        native_download_request(request, deadline)
    }
}

fn native_download_request(
    request: ureq::RequestBuilder<ureq::typestate::WithoutBody>,
    deadline: Instant,
) -> Result<ToolDownloadResponse, OrchestratorError> {
    let invalid = || internal("crate_release_tool_download_transport");
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .filter(|duration| !duration.is_zero())
        .ok_or_else(invalid)?;
    let response = request
        .config()
        .timeout_global(Some(remaining))
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .call()
        .map_err(|_| invalid())?;
    let headers = response.headers();
    let single = |name: &str| -> Result<Option<String>, OrchestratorError> {
        let mut values = headers.get_all(name).iter();
        let first = values
            .next()
            .map(|value| value.to_str().map(str::to_owned).map_err(|_| invalid()))
            .transpose()?;
        if values.next().is_some() {
            return Err(invalid());
        }
        Ok(first)
    };
    let location = single("location")?;
    let content_encoding = single("content-encoding")?;
    let content_length = single("content-length")?
        .map(|value| value.parse().map_err(|_| invalid()))
        .transpose()?;
    let status = response.status().as_u16();
    Ok(ToolDownloadResponse {
        status,
        location,
        content_length,
        content_encoding,
        body: Box::new(response.into_body().into_reader()),
    })
}

#[cfg(test)]
mod tests {
    use super::native_download_request;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
        time::{Duration, Instant},
    };

    fn reply_once(response: &'static str) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listen");
        let address = listener.local_addr().expect("address");
        listener.set_nonblocking(true).expect("nonblocking");
        let worker = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let (mut socket, _) = loop {
                match listener.accept() {
                    Ok(connection) => break connection,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("expected request: {error}"),
                }
            };
            socket.set_nonblocking(false).expect("blocking");
            let mut bytes = [0u8; 4096];
            let n = socket.read(&mut bytes).expect("read");
            socket.write_all(response.as_bytes()).expect("write");
            String::from_utf8_lossy(&bytes[..n]).into_owned()
        });
        (format!("http://{address}/archive"), worker)
    }

    #[test]
    fn native_transport_does_not_follow_redirect_or_add_credentials() {
        let (url, worker) = reply_once(
            "HTTP/1.1 302 Found\r\nLocation: https://example.blob.core.windows.net/file\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
        let request = crate::api::github_evidence_http::agent()
            .expect("agent")
            .get(url);
        let response = native_download_request(request, Instant::now() + Duration::from_secs(5))
            .expect("response");
        assert_eq!(response.status, 302);
        let request = worker.join().expect("worker").to_ascii_lowercase();
        assert!(!request.contains("authorization:"));
    }

    #[test]
    fn native_transport_rejects_duplicate_security_headers() {
        let (url, worker) = reply_once(
            "HTTP/1.1 200 OK\r\nContent-Length: 1\r\nContent-Length: 2\r\nConnection: close\r\n\r\na",
        );
        let request = crate::api::github_evidence_http::agent()
            .expect("agent")
            .get(url);
        assert!(native_download_request(request, Instant::now() + Duration::from_secs(5)).is_err());
        worker.join().expect("worker");
    }
}
