use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use tokio::sync::OwnedSemaphorePermit;
use velnor_runner_github::{Exchange, Method, SessionRequest, TransportFail};
use zeroize::Zeroizing;

use process::CurlChild;

use super::STATUS_MARKER;

#[path = "discovery_curl_process.rs"]
mod process;
#[path = "discovery_curl_readers.rs"]
mod readers;

pub(in crate::https) use readers::{BodyReadError, read_bounded};

#[cfg(test)]
pub(super) fn perform_curl(
    executable: &str,
    url: &str,
    request: &SessionRequest,
    body_limit: usize,
    deadline: Duration,
) -> Result<Exchange, TransportFail> {
    let permit = Arc::new(super::try_discovery_worker_permit()?);
    let cancellation = AtomicBool::new(false);
    perform_curl_cancellable_with_permit(
        executable,
        url,
        request,
        body_limit,
        deadline,
        &cancellation,
        permit,
    )
}

#[cfg(test)]
pub(super) fn perform_curl_cancellable(
    executable: &str,
    url: &str,
    request: &SessionRequest,
    body_limit: usize,
    deadline: Duration,
    cancellation: &AtomicBool,
) -> Result<Exchange, TransportFail> {
    let permit = Arc::new(super::try_discovery_worker_permit()?);
    perform_curl_cancellable_with_permit(
        executable,
        url,
        request,
        body_limit,
        deadline,
        cancellation,
        permit,
    )
}

#[cfg(test)]
fn perform_curl_cancellable_with_permit(
    executable: &str,
    url: &str,
    request: &SessionRequest,
    body_limit: usize,
    deadline: Duration,
    cancellation: &AtomicBool,
    permit: Arc<OwnedSemaphorePermit>,
) -> Result<Exchange, TransportFail> {
    let stop_at = Instant::now()
        .checked_add(deadline)
        .ok_or(TransportFail::Timeout)?;
    perform_curl_until_cancellable_with_permit(
        executable,
        url,
        request,
        body_limit,
        stop_at,
        cancellation,
        permit,
    )
}

#[cfg(test)]
pub(super) fn perform_curl_until_cancellable(
    executable: &str,
    url: &str,
    request: &SessionRequest,
    body_limit: usize,
    stop_at: Instant,
    cancellation: &AtomicBool,
) -> Result<Exchange, TransportFail> {
    let permit = Arc::new(super::try_discovery_worker_permit()?);
    perform_curl_until_cancellable_with_permit(
        executable,
        url,
        request,
        body_limit,
        stop_at,
        cancellation,
        permit,
    )
}

pub(super) fn perform_curl_until_cancellable_with_permit(
    executable: &str,
    url: &str,
    request: &SessionRequest,
    body_limit: usize,
    stop_at: Instant,
    cancellation: &AtomicBool,
    permit: Arc<OwnedSemaphorePermit>,
) -> Result<Exchange, TransportFail> {
    if cancellation.load(Ordering::Acquire) {
        return Err(TransportFail::Reset);
    }
    let remaining = stop_at.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(TransportFail::Timeout);
    }
    let cleanup_budget = Duration::from_millis(250).min(remaining / 4);
    let work_stop_at = stop_at.checked_sub(cleanup_budget).unwrap_or(stop_at);
    if Instant::now() >= work_stop_at {
        return Err(TransportFail::Timeout);
    }
    let config = curl_config(
        url,
        request,
        work_stop_at.saturating_duration_since(Instant::now()),
    )
    .ok_or(TransportFail::Reset)?;
    if cancellation.load(Ordering::Acquire) {
        return Err(TransportFail::Reset);
    }
    if Instant::now() >= work_stop_at {
        return Err(TransportFail::Timeout);
    }
    let mut process = CurlChild::spawn(executable, body_limit, stop_at, permit)?;
    process.start_config(config, work_stop_at, cancellation)?;
    process.finish(work_stop_at, cancellation)
}

pub(super) fn curl_args() -> [&'static str; 9] {
    [
        "--disable",
        "--silent",
        "--no-location",
        "--proto",
        curl_protocols(),
        "--connect-timeout",
        "5",
        "--config",
        "-",
    ]
}

#[cfg(not(test))]
const fn curl_protocols() -> &'static str {
    "=https"
}

#[cfg(test)]
const fn curl_protocols() -> &'static str {
    "=http,https"
}

fn curl_config(
    url: &str,
    request: &SessionRequest,
    remaining: Duration,
) -> Option<Zeroizing<String>> {
    let mut lines: Vec<Zeroizing<String>> = Vec::with_capacity(request.headers.len() + 5);
    lines.push(quoted_line("request", method_name(request.method))?);
    lines.push(quoted_line("url", url)?);
    lines.push(quoted_line(
        "write-out",
        &format!("%{{stderr}}{STATUS_MARKER}%{{http_code}}"),
    )?);
    lines.push(quoted_line(
        "max-time",
        &format!("{:.3}", remaining.as_secs_f64().max(0.001)),
    )?);
    for (name, value) in &request.headers {
        let mut header = Zeroizing::new(String::with_capacity(name.len() + value.len() + 2));
        header.push_str(name);
        header.push_str(": ");
        header.push_str(value);
        lines.push(quoted_line("header", &header)?);
    }
    if !request.body.is_empty() {
        let body = std::str::from_utf8(&request.body).ok()?;
        lines.push(quoted_line("data-binary", body)?);
    }
    let mut rendered = String::new();
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            rendered.push('\n');
        }
        rendered.push_str(line);
    }
    Some(Zeroizing::new(rendered))
}

fn quoted_line(key: &str, value: &str) -> Option<Zeroizing<String>> {
    let mut line = Zeroizing::new(format!("{key} = "));
    quote_into(&mut line, value)?;
    Some(line)
}

fn quote_into(target: &mut String, value: &str) -> Option<()> {
    if value.bytes().any(|byte| byte.is_ascii_control()) {
        return None;
    }
    target.push('"');
    for ch in value.chars() {
        if matches!(ch, '"' | '\\') {
            target.push('\\');
        }
        target.push(ch);
    }
    target.push('"');
    Some(())
}

const fn method_name(method: Method) -> &'static str {
    match method {
        Method::Get => "GET",
        Method::Post => "POST",
        Method::Delete => "DELETE",
        Method::Patch => "PATCH",
    }
}

#[cfg(test)]
#[path = "discovery_curl_tests.rs"]
mod tests;
