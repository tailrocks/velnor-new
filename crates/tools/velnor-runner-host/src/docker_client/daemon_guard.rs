//! One-connection Docker daemon identity guard.

use std::error::Error as StdError;
use std::time::Duration;

use http_body_util::{BodyExt, Empty};
use hyper::body::{Body, Bytes, Incoming};
use hyper::client::conn::http1;
use hyper::header::{ACCEPT, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, HOST};
use hyper::{Method, Request, Response, StatusCode, Uri};
use hyper_util::rt::TokioIo;
use serde::Deserialize;
use tokio::net::UnixStream;
use tokio::task::JoinHandle;
use tokio::time::{Instant, timeout_at};

use crate::HostError;
use crate::docker_client::DockerDaemonBinding;

const MAX_DAEMON_INFO_BYTES: usize = 256 * 1024;
const MAX_DAEMON_ID_BYTES: usize = 1024;
const DEFAULT_DOCKER_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

type EmptyBody = Empty<Bytes>;

/// Construct a Bollard client that verifies identity and sends each original request
/// over the same accepted Unix socket connection.
pub(super) fn connect_bound(binding: &DockerDaemonBinding) -> Result<bollard::Docker, HostError> {
    validate_engine_id(binding.engine_id())?;
    let endpoint = binding.endpoint().to_owned();
    let expected_id = binding.engine_id().to_owned();
    bollard::Docker::connect_with_custom_transport(
        move |request: bollard::BollardRequest| {
            let endpoint = endpoint.clone();
            let expected_id = expected_id.clone();
            Box::pin(async move {
                let deadline = Instant::now() + DEFAULT_DOCKER_REQUEST_TIMEOUT;
                guarded_request(
                    &endpoint,
                    &expected_id,
                    request,
                    || bollard::body_full(Bytes::new()),
                    deadline,
                )
                .await
                .map_err(|_| sanitized_docker_error())
            })
        },
        Some("http://localhost"),
        DEFAULT_DOCKER_REQUEST_TIMEOUT.as_secs(),
        bollard::API_DEFAULT_VERSION,
    )
    .map_err(|_| HostError::Docker)
}

/// Perform the only unbound Docker request: observational `/info` before effects.
pub(super) async fn observe_engine_id(
    endpoint: &str,
    deadline: Instant,
) -> Result<String, HostError> {
    ensure_before_deadline(deadline)?;
    timeout_at(deadline, async {
        let stream = UnixStream::connect(endpoint)
            .await
            .map_err(|_| HostError::Docker)?;
        let (mut sender, connection) = http1::handshake(TokioIo::new(stream))
            .await
            .map_err(|_| HostError::Docker)?;
        let mut driver = DriverGuard::new(tokio::spawn(async move {
            drop(connection.with_upgrades().await);
        }));

        let request = info_request::<EmptyBody>(None, Empty::new())?;
        let response = send_before_deadline(&mut sender, request, deadline).await?;
        let engine_id = parse_info_response(response, deadline).await?;
        driver.detach();
        Ok(engine_id)
    })
    .await
    .map_err(|_| HostError::Docker)?
}

/// Verify `/info` and dispatch exactly one original request on that same connection.
pub(crate) async fn guarded_request<B, EmptyBodyFactory>(
    endpoint: &str,
    expected_engine_id: &str,
    request: Request<B>,
    empty_body: EmptyBodyFactory,
    deadline: Instant,
) -> Result<Response<Incoming>, HostError>
where
    B: Body<Data = Bytes> + Send + 'static,
    B::Error: Into<Box<dyn StdError + Send + Sync>> + 'static,
    EmptyBodyFactory: FnOnce() -> B,
{
    validate_engine_id(expected_engine_id)?;
    ensure_before_deadline(deadline)?;
    timeout_at(deadline, async move {
        let version = api_version_segment(request.uri())?;
        let original_request = local_request_uri(request)?;
        let stream = UnixStream::connect(endpoint)
            .await
            .map_err(|_| HostError::Docker)?;
        let (mut sender, connection) = http1::handshake(TokioIo::new(stream))
            .await
            .map_err(|_| HostError::Docker)?;
        let mut driver = DriverGuard::new(tokio::spawn(async move {
            drop(connection.with_upgrades().await);
        }));

        let request = info_request(version.as_deref(), empty_body())?;
        let response = send_before_deadline(&mut sender, request, deadline).await?;
        let observed_id = parse_info_response(response, deadline).await?;
        if observed_id != expected_engine_id {
            return Err(HostError::Docker);
        }

        let response = send_before_deadline(&mut sender, original_request, deadline).await?;
        driver.detach();
        Ok(response)
    })
    .await
    .map_err(|_| HostError::Docker)?
}

async fn send_before_deadline<B>(
    sender: &mut http1::SendRequest<B>,
    request: Request<B>,
    deadline: Instant,
) -> Result<Response<Incoming>, HostError>
where
    B: Body<Data = Bytes> + Send + 'static,
    B::Error: Into<Box<dyn StdError + Send + Sync>> + 'static,
{
    sender.ready().await.map_err(|_| HostError::Docker)?;
    ensure_before_deadline(deadline)?;
    sender
        .send_request(request)
        .await
        .map_err(|_| HostError::Docker)
}

fn ensure_before_deadline(deadline: Instant) -> Result<(), HostError> {
    if Instant::now() >= deadline {
        Err(HostError::Docker)
    } else {
        Ok(())
    }
}

async fn parse_info_response(
    response: Response<Incoming>,
    deadline: Instant,
) -> Result<String, HostError> {
    if response.status() != StatusCode::OK
        || !is_json_content_type(response.headers())
        || is_unsupported_content_encoding(response.headers())
    {
        return Err(HostError::Docker);
    }
    reject_declared_oversize(response.headers())?;
    if response
        .body()
        .size_hint()
        .upper()
        .is_some_and(|length| length > MAX_DAEMON_INFO_BYTES as u64)
    {
        return Err(HostError::Docker);
    }

    let mut body = response.into_body();
    let mut bytes = Vec::new();
    while let Some(frame) = timeout_at(deadline, body.frame())
        .await
        .map_err(|_| HostError::Docker)?
    {
        let frame = frame.map_err(|_| HostError::Docker)?;
        let data = frame.into_data().map_err(|_| HostError::Docker)?;
        if data.len() > MAX_DAEMON_INFO_BYTES.saturating_sub(bytes.len()) {
            return Err(HostError::Docker);
        }
        bytes
            .try_reserve(data.len())
            .map_err(|_| HostError::Docker)?;
        bytes.extend_from_slice(&data);
    }
    if Instant::now() >= deadline {
        return Err(HostError::Docker);
    }

    let decoded: DockerInfoIdentity =
        serde_json::from_slice(&bytes).map_err(|_| HostError::Docker)?;
    if Instant::now() >= deadline {
        return Err(HostError::Docker);
    }
    validate_engine_id(&decoded.id)?;
    Ok(decoded.id)
}

fn info_request<B>(version: Option<&str>, body: B) -> Result<Request<B>, HostError> {
    let path = version.map_or_else(|| "/info".to_owned(), |version| format!("/{version}/info"));
    let uri: Uri = path.parse().map_err(|_| HostError::Docker)?;
    Request::builder()
        .method(Method::GET)
        .uri(uri)
        .header(HOST, "localhost")
        .header(ACCEPT, "application/json")
        .body(body)
        .map_err(|_| HostError::Docker)
}

fn api_version_segment(uri: &Uri) -> Result<Option<String>, HostError> {
    let path = uri.path();
    let segment = path.split('/').nth(1).ok_or(HostError::Docker)?;
    let Some(version) = segment.strip_prefix('v') else {
        return Ok(None);
    };
    let (major, minor) = version.split_once('.').ok_or(HostError::Docker)?;
    if major.is_empty()
        || minor.is_empty()
        || !major.bytes().all(|byte| byte.is_ascii_digit())
        || !minor.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(HostError::Docker);
    }
    Ok(Some(segment.to_owned()))
}

fn local_request_uri<B>(request: Request<B>) -> Result<Request<B>, HostError> {
    let target = request
        .uri()
        .path_and_query()
        .ok_or(HostError::Docker)?
        .as_str();
    if !target.starts_with('/') || target.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(HostError::Docker);
    }
    let uri: Uri = target.parse().map_err(|_| HostError::Docker)?;
    let (mut parts, body) = request.into_parts();
    parts.uri = uri;
    parts
        .headers
        .insert(HOST, hyper::header::HeaderValue::from_static("localhost"));
    Ok(Request::from_parts(parts, body))
}

fn is_json_content_type(headers: &hyper::HeaderMap) -> bool {
    let mut values = headers.get_all(CONTENT_TYPE).iter();
    let Some(value) = values.next() else {
        return false;
    };
    if values.next().is_some() {
        return false;
    }
    value.to_str().is_ok_and(|value| {
        value
            .split(';')
            .next()
            .is_some_and(|media_type| media_type.trim().eq_ignore_ascii_case("application/json"))
    })
}

fn is_unsupported_content_encoding(headers: &hyper::HeaderMap) -> bool {
    let mut values = headers.get_all(CONTENT_ENCODING).iter();
    let Some(value) = values.next() else {
        return false;
    };
    if values.next().is_some() {
        return true;
    }
    !value
        .to_str()
        .is_ok_and(|encoding| encoding.eq_ignore_ascii_case("identity"))
}

fn reject_declared_oversize(headers: &hyper::HeaderMap) -> Result<(), HostError> {
    let mut lengths = headers.get_all(CONTENT_LENGTH).iter();
    let Some(value) = lengths.next() else {
        return Ok(());
    };
    if lengths.next().is_some() {
        return Err(HostError::Docker);
    }
    let length = value
        .to_str()
        .ok()
        .and_then(|text| text.parse::<u64>().ok())
        .ok_or(HostError::Docker)?;
    if length > MAX_DAEMON_INFO_BYTES as u64 {
        return Err(HostError::Docker);
    }
    Ok(())
}

fn validate_engine_id(engine_id: &str) -> Result<(), HostError> {
    if engine_id.trim().is_empty()
        || engine_id.len() > MAX_DAEMON_ID_BYTES
        || engine_id.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(HostError::Docker);
    }
    Ok(())
}

fn sanitized_docker_error() -> bollard::errors::Error {
    bollard::errors::Error::DockerResponseServerError {
        status_code: 0,
        message: "bounded Docker request failed".to_owned(),
    }
}

#[derive(Deserialize)]
struct DockerInfoIdentity {
    #[serde(rename = "ID")]
    id: String,
}

struct DriverGuard(Option<JoinHandle<()>>);

impl DriverGuard {
    fn new(driver: JoinHandle<()>) -> Self {
        Self(Some(driver))
    }

    fn detach(&mut self) {
        drop(self.0.take());
    }
}

impl Drop for DriverGuard {
    fn drop(&mut self) {
        if let Some(driver) = self.0.take() {
            driver.abort();
        }
    }
}

#[cfg(test)]
#[path = "daemon_guard_tests.rs"]
mod tests;
