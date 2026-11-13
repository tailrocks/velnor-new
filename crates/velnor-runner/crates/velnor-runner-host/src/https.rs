// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use velnor_runner_github::{Exchange, Method, SessionRequest, Transport, TransportFail};

use crate::error::HostError;

mod process;
mod retry_after;

const MAX_RESPONSE_BODY_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpsTransport {
    base: String,
    max_time_seconds: u64,
    deadline: Option<Instant>,
}

impl HttpsTransport {
    pub fn new(base: &str) -> Result<Self, HostError> {
        Ok(Self {
            base: checked_base(base)?,
            max_time_seconds: 60,
            deadline: None,
        })
    }

    pub(crate) fn with_deadline(base: &str, deadline: Instant) -> Result<Self, HostError> {
        let transport = Self::new(base)?;
        Ok(transport.bounded_by_deadline(deadline))
    }

    #[must_use]
    pub(crate) fn bounded_by_deadline(&self, deadline: Instant) -> Self {
        Self {
            base: self.base.clone(),
            max_time_seconds: self.max_time_seconds,
            deadline: Some(
                self.deadline
                    .map_or(deadline, |current| current.min(deadline)),
            ),
        }
    }

    pub fn set_base(&mut self, base: &str) -> Result<(), HostError> {
        self.base = checked_base(base)?;
        Ok(())
    }
}

impl Transport for HttpsTransport {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        let deadline =
            request_deadline(self.deadline, self.max_time_seconds).ok_or(TransportFail::Timeout)?;
        let result = perform(&self.base, deadline, request);
        if self.deadline.is_some_and(|limit| Instant::now() >= limit) {
            return Err(TransportFail::Timeout);
        }
        match result {
            Ok(exchange) => Ok(exchange),
            Err(CurlFail::Timeout) => Err(TransportFail::Timeout),
            Err(CurlFail::Reset | CurlFail::ResponseTooLarge) => Err(TransportFail::Reset),
        }
    }
}

enum CurlFail {
    Timeout,
    Reset,
    ResponseTooLarge,
}

fn perform(base: &str, deadline: Instant, request: &SessionRequest) -> Result<Exchange, CurlFail> {
    perform_bounded(base, deadline, request, MAX_RESPONSE_BODY_BYTES)
}

fn perform_bounded(
    base: &str,
    deadline: Instant,
    request: &SessionRequest,
    max_response_bytes: usize,
) -> Result<Exchange, CurlFail> {
    let scratch = Scratch::create()?;
    perform_with_scratch(base, deadline, request, max_response_bytes, &scratch)
}

fn perform_with_scratch(
    base: &str,
    deadline: Instant,
    request: &SessionRequest,
    max_response_bytes: usize,
    scratch: &Scratch,
) -> Result<Exchange, CurlFail> {
    retry_after::wait_for_base(base, deadline)?;
    let url = join_url(base, &request.path, request.query.as_deref()).ok_or(CurlFail::Reset)?;
    let body_path = scratch.path("body");
    write_private(&body_path, &request.body)?;
    let max_time = remaining(deadline).ok_or(CurlFail::Timeout)?;
    let config = crate::curl_config::encode(&url, request, &body_path, max_time)
        .map_err(|_| CurlFail::Reset)?;
    let response = process::run_curl(config.as_str(), deadline, max_response_bytes)?;
    retry_after::record_response(base, response.status, response.retry_after.as_deref());
    if Instant::now() >= deadline {
        return Err(CurlFail::Timeout);
    }
    trace(base, request, response.status, response.body.len());
    Ok(Exchange {
        status: response.status,
        body: response.body,
    })
}

fn trace(base: &str, request: &SessionRequest, status: u16, response_bytes: usize) {
    if std::env::var_os("VELNOR_HTTPS_TRACE").is_none() {
        return;
    }
    for line in trace_lines(base, request, status, response_bytes) {
        eprintln!("{line}");
    }
}

pub(super) fn trace_lines(
    base: &str,
    request: &SessionRequest,
    status: u16,
    response_bytes: usize,
) -> Vec<String> {
    let host = base.split('/').nth(2).unwrap_or("");
    let verb = method_name(request.method);
    let mut lines = vec![format!(
        "trace host={host} verb={verb} path={} status={status} bytes={response_bytes}",
        request.path
    )];
    if status >= 400 {
        lines.push("trace body=omitted".to_owned());
    }
    lines
}

fn request_deadline(overall: Option<Instant>, max_time_seconds: u64) -> Option<Instant> {
    request_deadline_at(overall, max_time_seconds, Instant::now())
}

pub(super) fn request_deadline_at(
    overall: Option<Instant>,
    max_time_seconds: u64,
    now: Instant,
) -> Option<Instant> {
    let request = now.checked_add(Duration::from_secs(max_time_seconds))?;
    Some(overall.map_or(request, |deadline| deadline.min(request)))
}

fn remaining(deadline: Instant) -> Option<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|left| !left.is_zero())
}

fn classify_exit(code: Option<i32>, stdout: &[u8]) -> Result<u16, CurlFail> {
    match code {
        Some(0) => parse_status(stdout),
        Some(28) => Err(CurlFail::Timeout),
        _ => Err(CurlFail::Reset),
    }
}

fn parse_status(stdout: &[u8]) -> Result<u16, CurlFail> {
    let text = std::str::from_utf8(stdout).map_err(|_| CurlFail::Reset)?;
    let status: u16 = text.trim().parse().map_err(|_| CurlFail::Reset)?;
    if (100..600).contains(&status) {
        Ok(status)
    } else {
        Err(CurlFail::Reset)
    }
}

pub(crate) const CURL_ARGV: &[&str] = &[
    "-q",
    "--silent",
    "--show-error",
    "--config",
    "-",
    "--retry",
    "0",
];

pub(crate) const fn method_name(method: Method) -> &'static str {
    match method {
        Method::Get => "GET",
        Method::Post => "POST",
        Method::Delete => "DELETE",
        Method::Patch => "PATCH",
    }
}

pub(crate) fn join_url(base: &str, path: &str, query: Option<&str>) -> Option<String> {
    let path = path.trim_start_matches('/');
    let mut url = format!("{base}/{path}");
    if let Some(query) = query.filter(|text| !text.is_empty()) {
        if query
            .chars()
            .any(|ch| matches!(ch, '"' | '\n' | '\r' | '\0'))
        {
            return None;
        }
        url.push('?');
        url.push_str(query);
    }
    Some(url)
}

fn checked_base(base: &str) -> Result<String, HostError> {
    let trimmed = base.trim_end_matches('/');
    if trimmed.starts_with("https://")
        && trimmed.len() > "https://".len()
        && !trimmed
            .chars()
            .any(|ch| ch.is_whitespace() || matches!(ch, '"' | '\n' | '\r' | '\0' | '?' | '#'))
    {
        Ok(trimmed.to_owned())
    } else {
        Err(HostError::Endpoint)
    }
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), CurlFail> {
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|_| CurlFail::Reset)?;
    file.write_all(bytes).map_err(|_| CurlFail::Reset)?;
    Ok(())
}

struct Scratch {
    dir: PathBuf,
}

impl Scratch {
    fn create() -> Result<Self, CurlFail> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| CurlFail::Reset)?
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("velnor-https-{nanos}"));
        fs::create_dir(&dir).map_err(|_| CurlFail::Reset)?;
        let perms = std::os::unix::fs::PermissionsExt::from_mode(0o700);
        fs::set_permissions(&dir, perms).map_err(|_| CurlFail::Reset)?;
        Ok(Self { dir })
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.dir);
    }
}
