//! HTTPS through `curl`. Credentials travel on stdin, never on argv.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use velnor_runner_github::{Exchange, Method, SessionRequest, Transport, TransportFail};

use crate::error::HostError;

/// One Actions or GitHub API origin. The path on each call is relative.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpsTransport {
    base: String,
    max_time_seconds: u64,
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
        })
    }

    /// Clone this origin with a short deadline for background cleanup requests.
    #[must_use]
    #[cfg(test)]
    pub(crate) fn cleanup_client(&self) -> Self {
        Self {
            base: self.base.clone(),
            max_time_seconds: 5,
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
        match perform(&self.base, self.max_time_seconds, request) {
            Ok(exchange) => Ok(exchange),
            Err(CurlFail::Timeout) => Err(TransportFail::Timeout),
            Err(CurlFail::Reset) => Err(TransportFail::Reset),
        }
    }
}

enum CurlFail {
    Timeout,
    Reset,
}

fn perform(
    base: &str,
    max_time_seconds: u64,
    request: &SessionRequest,
) -> Result<Exchange, CurlFail> {
    let url = join_url(base, &request.path, request.query.as_deref()).ok_or(CurlFail::Reset)?;
    let scratch = Scratch::create()?;
    let body_path = scratch.path("body");
    let out_path = scratch.path("out");
    write_private(&body_path, &request.body)?;
    let config = curl_config(&url, request, &body_path, &out_path, max_time_seconds)?;
    let status = run_curl(&config)?;
    let body = read_output(&out_path)?;
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

fn run_curl(config: &str) -> Result<u16, CurlFail> {
    let mut child = Command::new("curl")
        .args(CURL_ARGV)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| CurlFail::Reset)?;
    let mut stdin = child.stdin.take().ok_or(CurlFail::Reset)?;
    stdin
        .write_all(config.as_bytes())
        .map_err(|_| CurlFail::Reset)?;
    drop(stdin);
    let finished = child.wait_with_output().map_err(|_| CurlFail::Reset)?;
    classify_exit(finished.status.code(), &finished.stdout)
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

pub(crate) const CURL_ARGV: &[&str] = &["--silent", "--show-error", "--config", "-"];

fn curl_config(
    url: &str,
    request: &SessionRequest,
    body: &Path,
    output: &Path,
    max_time_seconds: u64,
) -> Result<String, CurlFail> {
    let mut lines = vec![
        format!("request = \"{}\"", method_name(request.method)),
        quoted("url", url)?,
        format!("output = \"{}\"", display_path(output)?),
        "write-out = \"%{http_code}\"".to_owned(),
        format!("max-time = {max_time_seconds}"),
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

fn read_output(path: &Path) -> Result<Vec<u8>, CurlFail> {
    match fs::read(path) {
        Ok(bytes) => Ok(bytes),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(_) => Err(CurlFail::Reset),
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
