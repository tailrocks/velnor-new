//! HTTP transport with response-byte and decode-work bounds.

use std::sync::{Arc, OnceLock};

use http_body_util::{BodyExt, Empty};
use hyper::body::{Body, Bytes, Incoming};
use hyper::{Method, Request, StatusCode};
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use hyperlocal::{UnixConnector, Uri as UnixUri};
use serde::de::DeserializeOwned;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::time::{Instant, timeout_at};

use crate::HostError;
use crate::docker_client::{DockerDaemonBinding, daemon_guard::guarded_request, unix_socket_path};

const MAX_HTTP_BUFFER_BYTES: usize = 16 * 1024;
/// Maximum decoded JSON response bytes retained for any one inventory endpoint.
pub const MAX_INVENTORY_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

type RequestBody = Empty<Bytes>;
type InventoryClient = Client<UnixConnector, RequestBody>;

#[derive(Clone)]
enum InventoryTransport {
    Unbound {
        client: InventoryClient,
        socket_path: String,
    },
    Bound(DockerDaemonBinding),
}

/// One connection pool for the sequential list calls in one inventory query.
#[derive(Clone)]
pub(super) struct BoundedDockerApi {
    transport: InventoryTransport,
}

impl BoundedDockerApi {
    /// Create an inventory client for the already-configured Unix endpoint.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Docker`] for non-Unix, relative, or malformed endpoints.
    pub(super) fn new(endpoint: &str) -> Result<Self, HostError> {
        let socket_path = unix_socket_path(endpoint)?;
        let mut builder = Client::<(), ()>::builder(TokioExecutor::new());
        builder
            .http1_max_buf_size(MAX_HTTP_BUFFER_BYTES)
            .pool_max_idle_per_host(0);
        Ok(Self {
            transport: InventoryTransport::Unbound {
                client: builder.build::<_, RequestBody>(UnixConnector),
                socket_path,
            },
        })
    }

    /// Create a list client that guards every request with same-connection daemon identity.
    pub(super) fn new_bound(binding: &DockerDaemonBinding) -> Result<Self, HostError> {
        let _validated_endpoint = unix_socket_path(binding.endpoint())?;
        Ok(Self {
            transport: InventoryTransport::Bound(binding.clone()),
        })
    }

    /// Fetch and decode one fixed Docker Engine API JSON route under a shared absolute deadline.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Docker`] on transport, status, size, deadline, or JSON errors.
    pub(super) async fn get_json<T>(&self, route: &str, deadline: Instant) -> Result<T, HostError>
    where
        T: DeserializeOwned + Send + 'static,
    {
        self.get_json_with_limit(route, deadline, MAX_INVENTORY_RESPONSE_BYTES)
            .await
    }

    async fn get_json_with_limit<T>(
        &self,
        route: &str,
        deadline: Instant,
        byte_limit: usize,
    ) -> Result<T, HostError>
    where
        T: DeserializeOwned + Send + 'static,
    {
        let operation_permit = acquire_slot(decode_slots(), deadline).await?;
        let version = bollard::API_DEFAULT_VERSION;
        let path = format!(
            "/v{}.{}{}",
            version.major_version, version.minor_version, route
        );
        let uri: hyper::Uri = match &self.transport {
            InventoryTransport::Bound(_) => format!("http://localhost{path}")
                .parse()
                .map_err(|_| HostError::Docker)?,
            InventoryTransport::Unbound { socket_path, .. } => {
                UnixUri::new(socket_path, &path).into()
            }
        };
        let request = Request::builder()
            .method(Method::GET)
            .uri(uri)
            .header(hyper::header::ACCEPT, "application/json")
            .body(Empty::new())
            .map_err(|_| HostError::Docker)?;

        let bytes = timeout_at(deadline, async {
            let response = match &self.transport {
                InventoryTransport::Bound(binding) => {
                    guarded_request(
                        binding.endpoint(),
                        binding.engine_id(),
                        request,
                        Empty::new,
                        deadline,
                    )
                    .await?
                }
                InventoryTransport::Unbound { client, .. } => client
                    .request(request)
                    .await
                    .map_err(|_| HostError::Docker)?,
            };
            if response.status() != StatusCode::OK
                || !is_json_content_type(response.headers())
                || is_unsupported_content_encoding(response.headers())
            {
                return Err(HostError::Docker);
            }
            reject_declared_oversize(response.headers(), byte_limit)?;
            if response
                .body()
                .size_hint()
                .upper()
                .is_some_and(|length| length > byte_limit as u64)
            {
                return Err(HostError::Docker);
            }
            collect_bounded(response.into_body(), byte_limit).await
        })
        .await
        .map_err(|_| HostError::Docker)??;

        ensure_before_deadline(deadline)?;
        decode_json_until(bytes, deadline, operation_permit).await
    }
}

/// Serialize complete inventory snapshots so their partial output vectors do not overlap.
pub(super) async fn acquire_inventory_slot(
    deadline: Instant,
) -> Result<OwnedSemaphorePermit, HostError> {
    acquire_slot(inventory_slots(), deadline).await
}

async fn collect_bounded(mut body: Incoming, byte_limit: usize) -> Result<Vec<u8>, HostError> {
    let mut bytes = Vec::new();
    while let Some(frame) = body.frame().await {
        let frame = frame.map_err(|_| HostError::Docker)?;
        let data = frame.into_data().map_err(|_| HostError::Docker)?;
        if data.len() > byte_limit.saturating_sub(bytes.len()) {
            return Err(HostError::Docker);
        }
        bytes
            .try_reserve(data.len())
            .map_err(|_| HostError::Docker)?;
        bytes.extend_from_slice(&data);
    }
    Ok(bytes)
}

fn is_json_content_type(headers: &hyper::HeaderMap) -> bool {
    let mut values = headers.get_all(hyper::header::CONTENT_TYPE).iter();
    let Some(value) = values.next() else {
        return false;
    };
    if values.next().is_some() {
        return false;
    }
    value
        .to_str()
        .is_ok_and(|content_type| content_type.eq_ignore_ascii_case("application/json"))
}

fn is_unsupported_content_encoding(headers: &hyper::HeaderMap) -> bool {
    let mut values = headers.get_all(hyper::header::CONTENT_ENCODING).iter();
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

fn reject_declared_oversize(
    headers: &hyper::HeaderMap,
    byte_limit: usize,
) -> Result<(), HostError> {
    let mut lengths = headers.get_all(hyper::header::CONTENT_LENGTH).iter();
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
    if length > byte_limit as u64 {
        return Err(HostError::Docker);
    }
    Ok(())
}

async fn decode_json_until<T>(
    bytes: Vec<u8>,
    deadline: Instant,
    permit: OwnedSemaphorePermit,
) -> Result<T, HostError>
where
    T: DeserializeOwned + Send + 'static,
{
    decode_with_deadline(bytes, deadline, permit, |bytes| {
        serde_json::from_slice(&bytes).map_err(|_| HostError::Docker)
    })
    .await
}

async fn decode_with_deadline<T, F>(
    bytes: Vec<u8>,
    deadline: Instant,
    permit: OwnedSemaphorePermit,
    decode: F,
) -> Result<T, HostError>
where
    T: Send + 'static,
    F: FnOnce(Vec<u8>) -> Result<T, HostError> + Send + 'static,
{
    ensure_before_deadline(deadline)?;
    let (sender, receiver) = tokio::sync::oneshot::channel();
    // A detached thread avoids Tokio Runtime::drop waiting on an uncancellable blocking task.
    // Its permit remains held until decode ends, bounding post-deadline work to one payload.
    std::thread::Builder::new()
        .name("velnor-inventory-json".to_owned())
        .spawn(move || {
            let output = decode_with_permit(bytes, permit, decode);
            drop(sender.send(output));
        })
        .map_err(|_| HostError::Docker)?;
    receive_decode_result(receiver, deadline).await
}

async fn receive_decode_result<T>(
    receiver: tokio::sync::oneshot::Receiver<Result<T, HostError>>,
    deadline: Instant,
) -> Result<T, HostError> {
    ensure_before_deadline(deadline)?;
    let output = timeout_at(deadline, receiver)
        .await
        .map_err(|_| HostError::Docker)?
        .map_err(|_| HostError::Docker)?;
    ensure_before_deadline(deadline)?;
    output
}

async fn acquire_slot(
    slots: &Arc<Semaphore>,
    deadline: Instant,
) -> Result<OwnedSemaphorePermit, HostError> {
    ensure_before_deadline(deadline)?;
    let permit = timeout_at(deadline, Arc::clone(slots).acquire_owned())
        .await
        .map_err(|_| HostError::Docker)?
        .map_err(|_| HostError::Docker)?;
    ensure_before_deadline(deadline)?;
    Ok(permit)
}

pub(super) fn ensure_before_deadline(deadline: Instant) -> Result<(), HostError> {
    if Instant::now() >= deadline {
        Err(HostError::Docker)
    } else {
        Ok(())
    }
}

fn decode_with_permit<T, F>(bytes: Vec<u8>, permit: OwnedSemaphorePermit, decode: F) -> T
where
    F: FnOnce(Vec<u8>) -> T,
{
    let output = decode(bytes);
    drop(permit);
    output
}

// Serialize outstanding decoders, including ones whose caller already timed out.
fn decode_slots() -> &'static Arc<Semaphore> {
    static DECODE_SLOTS: OnceLock<Arc<Semaphore>> = OnceLock::new();
    DECODE_SLOTS.get_or_init(|| Arc::new(Semaphore::new(1)))
}

fn inventory_slots() -> &'static Arc<Semaphore> {
    static INVENTORY_SLOTS: OnceLock<Arc<Semaphore>> = OnceLock::new();
    INVENTORY_SLOTS.get_or_init(|| Arc::new(Semaphore::new(1)))
}

#[cfg(test)]
#[path = "transport/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "transport/bound_tests.rs"]
mod bound_tests;
