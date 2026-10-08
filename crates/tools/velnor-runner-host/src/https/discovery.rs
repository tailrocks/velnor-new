//! Bounded, origin-pinned transport for the credential-only discovery calls.

use std::sync::{Arc, OnceLock, atomic::AtomicBool};
use std::time::Duration;

use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use velnor_runner_github::{
    AsyncDiscoveryTransport, DiscoveryExchange, DiscoveryTransport, Exchange, MessageQueueRoute,
    SessionError, SessionRequest, Transport, TransportFail, WireError,
};
use zeroize::{Zeroize, Zeroizing};

use super::MAX_RESPONSE_BYTES;

const API_ORIGIN: &str = "https://api.github.com";
const MAX_REQUEST_BYTES: usize = 64 * 1024;
const MAX_HEADER_BYTES: usize = 32 * 1024;
const REQUEST_DEADLINE: Duration = Duration::from_secs(20);
const MAX_CONCURRENT_DISCOVERY_WORKERS: usize = 4;
const STATUS_MARKER: &str = "Velnor-HTTP:";

fn try_discovery_worker_permit() -> Result<OwnedSemaphorePermit, TransportFail> {
    static SLOTS: OnceLock<Arc<Semaphore>> = OnceLock::new();
    Arc::clone(SLOTS.get_or_init(|| Arc::new(Semaphore::new(MAX_CONCURRENT_DISCOVERY_WORKERS))))
        .try_acquire_owned()
        .map_err(|_| TransportFail::Reset)
}

/// Transport restricted to the exact API and GET routes used by auth-only
/// repository discovery. It is not a general-purpose HTTPS client.
pub struct BoundedDiscoveryTransport {
    origin: Option<Origin>,
    #[cfg(test)]
    test_base: Option<String>,
    curl: String,
    response_limit: usize,
    request_deadline: Duration,
}

enum Origin {
    GithubApi,
    Actions(String),
    MessageQueue(QueueOrigin),
}

struct QueueOrigin {
    base: String,
    route: MessageQueueRoute,
}

impl Drop for QueueOrigin {
    fn drop(&mut self) {
        self.base.zeroize();
    }
}

impl BoundedDiscoveryTransport {
    /// Create a transport with fixed deadlines and response limits.
    #[must_use]
    pub fn new() -> Self {
        Self {
            origin: None,
            #[cfg(test)]
            test_base: None,
            curl: "curl".to_owned(),
            response_limit: MAX_RESPONSE_BYTES,
            request_deadline: REQUEST_DEADLINE,
        }
    }

    fn bind_github(&mut self) {
        self.clear_origin();
        self.origin = Some(Origin::GithubApi);
    }

    fn bind_actions(&mut self, service_url: &str) -> Result<(), SessionError> {
        self.clear_origin();
        if service_url.len() > MAX_REQUEST_BYTES {
            return Err(WireError::RegistrationRejected.into());
        }
        let base = actions_base(service_url).ok_or(WireError::RegistrationRejected)?;
        self.origin = Some(Origin::Actions(base));
        Ok(())
    }

    fn bind_message_queue(&mut self, queue_url: &str) -> Result<MessageQueueRoute, SessionError> {
        self.clear_origin();
        if queue_url.len() > MAX_REQUEST_BYTES {
            return Err(WireError::RegistrationRejected.into());
        }
        let mut parts = queue_route(queue_url).ok_or(WireError::RegistrationRejected)?;
        let stored_route =
            match MessageQueueRoute::from_parts(parts.path.clone(), parts.query.clone()) {
                Ok(route) => route,
                Err(error) => {
                    parts.origin.zeroize();
                    parts.path.zeroize();
                    if let Some(query) = &mut parts.query {
                        query.zeroize();
                    }
                    return Err(error.into());
                }
            };
        let route = match MessageQueueRoute::from_parts(
            std::mem::take(&mut parts.path),
            parts.query.take(),
        ) {
            Ok(route) => route,
            Err(error) => {
                parts.origin.zeroize();
                parts.path.zeroize();
                if let Some(query) = &mut parts.query {
                    query.zeroize();
                }
                return Err(error.into());
            }
        };
        self.origin = Some(Origin::MessageQueue(QueueOrigin {
            base: std::mem::take(&mut parts.origin),
            route: stored_route,
        }));
        parts.origin.zeroize();
        parts.path.zeroize();
        if let Some(query) = &mut parts.query {
            query.zeroize();
        }
        Ok(route)
    }

    fn clear_origin(&mut self) {
        match &mut self.origin {
            Some(Origin::Actions(base)) => base.zeroize(),
            Some(Origin::MessageQueue(_) | Origin::GithubApi) | None => {}
        }
        self.origin = None;
    }

    fn exchange_bounded(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.exchange_until(request, None, &AtomicBool::new(false))
    }

    fn request_url(&self, request: &SessionRequest) -> Result<Zeroizing<String>, TransportFail> {
        if request.path.len() > MAX_REQUEST_BYTES
            || request
                .query
                .as_ref()
                .is_some_and(|query| query.len() > MAX_REQUEST_BYTES)
            || request.body.len() > MAX_REQUEST_BYTES
            || request.headers.len() > 8
        {
            return Err(TransportFail::Reset);
        }
        let origin = self.origin.as_ref().ok_or(TransportFail::Reset)?;
        let target = validate_discovery_request(origin, request).ok_or(TransportFail::Reset)?;
        let header_bytes = request
            .headers
            .iter()
            .try_fold(0usize, |total, (name, value)| {
                total
                    .checked_add(name.len())?
                    .checked_add(value.len())?
                    .checked_add(4)
            });
        if header_bytes.is_none_or(|bytes| bytes > MAX_HEADER_BYTES) {
            return Err(TransportFail::Reset);
        }
        #[cfg(test)]
        let test_base = self.test_base.as_deref();
        #[cfg(not(test))]
        let test_base = None;
        let base = Zeroizing::new(Self::request_base(origin, test_base));
        let capacity = base
            .len()
            .checked_add(target.path.len())
            .and_then(|size| size.checked_add(target.query.map_or(0, str::len)))
            .and_then(|size| size.checked_add(2))
            .filter(|size| *size <= MAX_REQUEST_BYTES)
            .ok_or(TransportFail::Reset)?;
        let mut url = String::with_capacity(capacity);
        url.push_str(base.trim_end_matches('/'));
        url.push('/');
        url.push_str(target.path);
        if let Some(query) = target.query {
            url.push('?');
            url.push_str(query);
        }
        if url.len() > MAX_REQUEST_BYTES {
            url.zeroize();
            return Err(TransportFail::Reset);
        }
        Ok(Zeroizing::new(url))
    }

    fn request_base(origin: &Origin, test_base: Option<&str>) -> String {
        #[cfg(test)]
        if let Some(test_base) = test_base {
            return match origin {
                Origin::GithubApi | Origin::MessageQueue(_) => test_base.to_owned(),
                Origin::Actions(service_base) => {
                    let path = service_base
                        .strip_prefix("https://")
                        .and_then(|rest| rest.split_once('/').map(|(_, path)| path))
                        .unwrap_or("");
                    if path.is_empty() {
                        test_base.to_owned()
                    } else {
                        format!("{}/{path}", test_base.trim_end_matches('/'))
                    }
                }
            };
        }
        #[cfg(not(test))]
        let _ = test_base;
        match origin {
            Origin::GithubApi => API_ORIGIN.to_owned(),
            Origin::Actions(base) => base.clone(),
            Origin::MessageQueue(queue) => queue.base.clone(),
        }
    }

    #[cfg(test)]
    fn for_test(base: String, response_limit: usize, request_deadline: Duration) -> Self {
        Self {
            origin: None,
            test_base: Some(base),
            curl: "curl".to_owned(),
            response_limit,
            request_deadline,
        }
    }
}

impl Drop for BoundedDiscoveryTransport {
    fn drop(&mut self) {
        self.clear_origin();
    }
}

impl Default for BoundedDiscoveryTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for BoundedDiscoveryTransport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("BoundedDiscoveryTransport([origin and credentials redacted])")
    }
}

impl Transport for BoundedDiscoveryTransport {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.exchange_bounded(request)
    }
}

impl DiscoveryTransport for BoundedDiscoveryTransport {
    fn bind_github_api_origin(&mut self) -> Result<(), SessionError> {
        self.bind_github();
        Ok(())
    }

    fn bind_actions_service_origin(&mut self, url: &str) -> Result<(), SessionError> {
        self.bind_actions(url)
    }

    fn bind_message_queue_origin(&mut self, url: &str) -> Result<MessageQueueRoute, SessionError> {
        self.bind_message_queue(url)
    }
}

impl AsyncDiscoveryTransport for BoundedDiscoveryTransport {
    fn bind_github_api_origin(&mut self) -> Result<(), SessionError> {
        self.bind_github();
        Ok(())
    }

    fn bind_actions_service_origin(&mut self, url: &str) -> Result<(), SessionError> {
        self.bind_actions(url)
    }

    fn exchange_discovery(&mut self, request: SessionRequest) -> DiscoveryExchange {
        self.exchange_discovery_until(request, None, Arc::new(AtomicBool::new(false)))
    }
}

#[path = "discovery_curl.rs"]
mod curl;
pub(in crate::https) use curl::{BodyReadError, read_bounded};
#[path = "discovery_queue_origin.rs"]
mod queue_origin;
#[path = "discovery_validation.rs"]
mod validation;

use queue_origin::queue_route;
use validation::{actions_base, validate_discovery_request};

#[path = "discovery_cutoff.rs"]
mod cutoff;

#[cfg(test)]
#[path = "discovery_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "discovery_actions_delete_tests.rs"]
mod actions_delete_tests;
#[cfg(test)]
#[path = "discovery_actions_provider_tests.rs"]
mod actions_provider_tests;
#[cfg(test)]
#[path = "discovery_queue_origin_tests.rs"]
mod queue_origin_tests;

#[cfg(test)]
#[path = "discovery_deadline_tests.rs"]
mod deadline_tests;
