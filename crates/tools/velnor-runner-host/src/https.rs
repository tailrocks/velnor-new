//! HTTPS through `curl`. Credentials travel on stdin, never on argv.

mod discovery;

pub use discovery::BoundedDiscoveryTransport;

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use velnor_runner_github::{Exchange, Method, SessionRequest, Transport, TransportFail};
use zeroize::Zeroize;

use crate::HostError;
use discovery::{BodyReadError, read_bounded};

const MAX_RESPONSE_BYTES: usize = 512 * 1024;
const HTTP_STATUS_BYTES: usize = 3;

/// One Actions or GitHub API origin. The path on each call is relative.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpsTransport {
    base: String,
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
        })
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
        match perform(&self.base, request) {
            Ok(exchange) => Ok(exchange),
            Err(CurlFail::Timeout) => Err(TransportFail::Timeout),
            Err(CurlFail::Reset | CurlFail::ResponseTooLarge) => Err(TransportFail::Reset),
        }
    }
}

#[derive(Debug)]
enum CurlFail {
    Timeout,
    Reset,
    ResponseTooLarge,
}

fn perform(base: &str, request: &SessionRequest) -> Result<Exchange, CurlFail> {
    let url = join_url(base, &request.path, request.query.as_deref()).ok_or(CurlFail::Reset)?;
    let scratch = Scratch::create()?;
    perform_in_scratch(&url, request, scratch, run_curl)
}

fn perform_in_scratch(
    url: &str,
    request: &SessionRequest,
    scratch: Scratch,
    run: impl FnOnce(&str) -> Result<(u16, Vec<u8>), CurlFail>,
) -> Result<Exchange, CurlFail> {
    let result = (|| {
        let body_path = scratch.path("body");
        write_private(&body_path, &request.body)?;
        let config = curl_config(url, request, &body_path)?;
        let (status, body) = run(&config)?;
        trace(request, status, &body);
        Ok(Exchange { status, body })
    })();
    scratch.finish(result)
}

fn trace(request: &SessionRequest, status: u16, body: &[u8]) {
    if std::env::var_os("VELNOR_HTTPS_TRACE").is_none() {
        return;
    }
    eprintln!("{}", trace_record(request, status, body));
}

fn trace_record(request: &SessionRequest, status: u16, body: &[u8]) -> String {
    let verb = method_name(request.method);
    let class = match status {
        100..=199 => "informational",
        200..=299 => "success",
        300..=399 => "redirect",
        400..=499 => "client_error",
        500..=599 => "server_error",
        _ => "invalid_status",
    };
    format!(
        "trace verb={verb} status={status} class={class} bytes={}",
        body.len()
    )
}

fn run_curl(config: &str) -> Result<(u16, Vec<u8>), CurlFail> {
    run_curl_with_executable("curl", config)
}

fn run_curl_with_executable(
    executable: impl AsRef<std::ffi::OsStr>,
    config: &str,
) -> Result<(u16, Vec<u8>), CurlFail> {
    let mut child = Command::new(executable)
        .args(CURL_ARGV)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| CurlFail::Reset)?;
    let result = (|| {
        let mut stdin = child.stdin.take().ok_or(CurlFail::Reset)?;
        let write_result = stdin.write_all(config.as_bytes());
        drop(stdin);
        write_result.map_err(|_| CurlFail::Reset)?;

        let stdout = child.stdout.take().ok_or(CurlFail::Reset)?;
        let output = match read_bounded(stdout, MAX_RESPONSE_BYTES + HTTP_STATUS_BYTES) {
            Ok(output) => output,
            Err(BodyReadError::TooLarge) => return Err(CurlFail::ResponseTooLarge),
            Err(BodyReadError::Io) => return Err(CurlFail::Reset),
        };
        let status = child.wait().map_err(|_| CurlFail::Reset)?.code();
        classify_exit(status, output)
    })();
    if result.is_err() {
        stop_and_reap(&mut child);
    }
    result
}

fn stop_and_reap(child: &mut Child) {
    drop(child.kill());
    drop(child.wait());
}

fn classify_exit(code: Option<i32>, mut stdout: Vec<u8>) -> Result<(u16, Vec<u8>), CurlFail> {
    match code {
        Some(0) => {
            let Some(status_start) = stdout.len().checked_sub(HTTP_STATUS_BYTES) else {
                stdout.zeroize();
                return Err(CurlFail::Reset);
            };
            let status = match parse_status(&stdout[status_start..]) {
                Ok(status) => status,
                Err(error) => {
                    stdout.zeroize();
                    return Err(error);
                }
            };
            stdout.truncate(status_start);
            if stdout.len() > MAX_RESPONSE_BYTES {
                stdout.zeroize();
                return Err(CurlFail::ResponseTooLarge);
            }
            Ok((status, stdout))
        }
        Some(28) => {
            stdout.zeroize();
            Err(CurlFail::Timeout)
        }
        Some(63) => {
            stdout.zeroize();
            Err(CurlFail::ResponseTooLarge)
        }
        _ => {
            stdout.zeroize();
            Err(CurlFail::Reset)
        }
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

fn curl_config(url: &str, request: &SessionRequest, body: &Path) -> Result<String, CurlFail> {
    let mut lines = vec![
        format!("request = \"{}\"", method_name(request.method)),
        quoted("url", url)?,
        "output = \"-\"".to_owned(),
        "write-out = \"%{http_code}\"".to_owned(),
        "max-time = 60".to_owned(),
        format!("max-filesize = {MAX_RESPONSE_BYTES}"),
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
        Self::create_in(&std::env::temp_dir())
    }

    fn create_in(parent: &Path) -> Result<Self, CurlFail> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| CurlFail::Reset)?
            .as_nanos();
        let dir = parent.join(format!("velnor-https-{nanos}"));
        fs::create_dir(&dir).map_err(|_| CurlFail::Reset)?;
        let scratch = Self { dir };
        let perms = std::os::unix::fs::PermissionsExt::from_mode(0o700);
        if fs::set_permissions(&scratch.dir, perms).is_err() {
            return scratch.finish(Err(CurlFail::Reset));
        }
        Ok(scratch)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    fn finish<T>(self, result: Result<T, CurlFail>) -> Result<T, CurlFail> {
        let cleanup_succeeded = match fs::remove_dir_all(&self.dir) {
            Ok(()) => true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
            Err(_) => false,
        };
        match result {
            Err(primary) => Err(primary),
            Ok(value) if cleanup_succeeded => Ok(value),
            Ok(_) => Err(CurlFail::Reset),
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.dir);
    }
}

#[cfg(test)]
mod tests;
