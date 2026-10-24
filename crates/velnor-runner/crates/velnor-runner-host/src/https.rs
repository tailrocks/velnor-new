//! HTTPS through `curl`. Credentials travel on stdin, never on argv.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use velnor_runner_github::{Exchange, Method, SessionRequest, Transport, TransportFail};

use crate::error::HostError;

#[cfg(test)]
mod deadline_tests;
mod process;
#[cfg(test)]
mod test_support;
#[cfg(test)]
pub(super) use test_support::exchange_fixture_response;

/// Limit each response body to one MiB.
const MAX_RESPONSE_BODY_BYTES: usize = 1024 * 1024;

/// One Actions or GitHub API origin. The path on each call is relative.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpsTransport {
    base: String,
    max_time_seconds: u64,
    deadline: Option<Instant>,
}

impl HttpsTransport {
    /// `base` must be `https://` with no quotes or whitespace.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Endpoint`] when `base` is not a usable origin.
    pub fn new(base: &str) -> Result<Self, HostError> {
        Ok(Self {
            base: checked_base(base)?,
            max_time_seconds: 60,
            deadline: None,
        })
    }

    /// Create a transport that shares one absolute deadline across its requests.
    pub(crate) fn with_deadline(base: &str, deadline: Instant) -> Result<Self, HostError> {
        let transport = Self::new(base)?;
        Ok(transport.bounded_by_deadline(deadline))
    }

    /// Return a copy of this transport without increasing its existing time limits.
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

    /// Clone this origin with a short deadline for background cleanup requests.
    #[must_use]
    #[cfg(test)]
    pub(crate) fn cleanup_client(&self) -> Self {
        Self {
            base: self.base.clone(),
            max_time_seconds: 5,
            deadline: self.deadline,
        }
    }

    /// Maximum curl operation time in seconds.
    #[must_use]
    #[cfg(test)]
    pub(crate) const fn timeout_seconds(&self) -> u64 {
        self.max_time_seconds
    }

    /// Replace the origin after the admin exchange returns the service URL.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Endpoint`] when `base` is not a usable origin.
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
    let url = join_url(base, &request.path, request.query.as_deref()).ok_or(CurlFail::Reset)?;
    let body_path = scratch.path("body");
    write_private(&body_path, &request.body)?;
    let max_time = remaining(deadline).ok_or(CurlFail::Timeout)?;
    let config = curl_config(&url, request, &body_path, max_time)?;
    let (status, body) = process::run_curl(&config, deadline, max_response_bytes)?;
    if Instant::now() >= deadline {
        return Err(CurlFail::Timeout);
    }
    trace(base, request, status, body.len());
    Ok(Exchange { status, body })
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

#[cfg(test)]
pub(super) fn run_curl_in_home(config: &str, home: &Path) -> Result<u16, &'static str> {
    process::run_in_home(config, home).map_err(|_| "curl failed")
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

// curl only honors -q when it is the first argument. This prevents an
// operator's .curlrc from adding retries or changing mutating request behavior.
pub(crate) const CURL_ARGV: &[&str] = &[
    "-q",
    "--silent",
    "--show-error",
    "--config",
    "-",
    "--retry",
    "0",
];

fn curl_config(
    url: &str,
    request: &SessionRequest,
    body: &Path,
    max_time: Duration,
) -> Result<String, CurlFail> {
    let max_time_seconds = max_time.as_secs_f64();
    let mut lines = vec![
        format!("request = \"{}\"", method_name(request.method)),
        quoted("url", url)?,
        "output = \"-\"".to_owned(),
        "write-out = \"%{http_code}\"".to_owned(),
        format!("max-time = {max_time_seconds:.3}"),
    ];
    for (name, value) in &request.headers {
        let header = format!("{name}: {value}");
        lines.push(quoted("header", &header)?);
    }
    if sends_body(request.method) {
        lines.push(format!("data-binary = \"@{}\"", display_path(body)?));
    }
    Ok(lines.join("\n"))
}

fn sends_body(method: Method) -> bool {
    matches!(method, Method::Post | Method::Patch)
}

const fn method_name(method: Method) -> &'static str {
    match method {
        Method::Get => "GET",
        Method::Post => "POST",
        Method::Delete => "DELETE",
        Method::Patch => "PATCH",
    }
}

fn quoted(key: &str, value: &str) -> Result<String, CurlFail> {
    if value
        .chars()
        .any(|ch| matches!(ch, '"' | '\n' | '\r' | '\0'))
    {
        return Err(CurlFail::Reset);
    }
    Ok(format!("{key} = \"{value}\""))
}

fn display_path(path: &Path) -> Result<String, CurlFail> {
    let text = path.to_str().ok_or(CurlFail::Reset)?;
    if text
        .chars()
        .any(|ch| matches!(ch, '"' | '\n' | '\r' | '\0'))
    {
        return Err(CurlFail::Reset);
    }
    Ok(text.to_owned())
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
