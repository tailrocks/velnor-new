use std::{
    fmt,
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
};

use crate::{Exchange, SessionError, SessionRequest, TransportFail};

/// One owned exchange future with cooperative cancellation on drop.
///
/// The host bridge should spawn its bounded synchronous process bridge on an
/// owned blocking worker, pass the same flag to that worker, and ensure the
/// worker kills/reaps its child and joins readers within its fixed deadline.
/// Dropping this value signals cancellation; it does not detach permission to
/// continue an unbounded request. A dropped POST's journal intent remains
/// Pending until separately reconciled.
#[must_use]
pub struct DiscoveryExchange {
    future: Pin<Box<dyn Future<Output = Result<Exchange, TransportFail>> + Send + 'static>>,
    cancellation: Option<Arc<AtomicBool>>,
}

impl fmt::Debug for DiscoveryExchange {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DiscoveryExchange([in progress])")
    }
}

impl DiscoveryExchange {
    /// Wrap an owned exchange future and the cancellation flag observed by its
    /// bounded worker.
    #[must_use = "the exchange must be polled or explicitly cancelled"]
    pub fn new<F>(future: F, cancellation: Arc<AtomicBool>) -> Self
    where
        F: Future<Output = Result<Exchange, TransportFail>> + Send + 'static,
    {
        Self {
            future: Box::pin(future),
            cancellation: Some(cancellation),
        }
    }
}

impl Future for DiscoveryExchange {
    type Output = Result<Exchange, TransportFail>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        match this.future.as_mut().poll(context) {
            Poll::Ready(result) => {
                // The network operation is terminal; dropping its wrapper
                // after this point must not request cancellation.
                this.cancellation.take();
                Poll::Ready(result)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl Drop for DiscoveryExchange {
    fn drop(&mut self) {
        if let Some(cancellation) = self.cancellation.take() {
            cancellation.store(true, Ordering::Release);
        }
    }
}

/// Async discovery transport. Origin binding is local validation only; the
/// request method must return promptly with an owned future and must not do
/// blocking I/O on the async executor.
///
/// Implementations must enforce the fixed GitHub API origin for bootstrap,
/// validate the returned Actions-service HTTPS origin before admin GETs,
/// reject redirects and automatic retries, cap response bytes while reading,
/// and use one whole-request deadline including child reaping and reader joins.
/// The GitHub API module supplies only fixed relative paths and queries.
pub trait AsyncDiscoveryTransport: Send {
    /// Bind the fixed GitHub API origin without sending a request.
    ///
    /// # Errors
    ///
    /// Returns a secret-safe error if the fixed API origin cannot be bound.
    fn bind_github_api_origin(&mut self) -> Result<(), SessionError>;

    /// Validate and bind the returned Actions-service HTTPS origin without
    /// sending a request.
    ///
    /// # Errors
    ///
    /// Returns a secret-safe error if the service URL does not satisfy the
    /// host's fixed-origin policy.
    fn bind_actions_service_origin(&mut self, url: &str) -> Result<(), SessionError>;

    /// Start one owned exchange on the bounded host worker. This API has no
    /// retry hook: every call represents exactly one transport attempt.
    fn exchange_discovery(&mut self, request: SessionRequest) -> DiscoveryExchange;
}

pub(in crate::registration) async fn execute_discovery<T>(
    transport: &mut T,
    request: SessionRequest,
) -> Result<Exchange, SessionError>
where
    T: AsyncDiscoveryTransport + ?Sized,
{
    match transport.exchange_discovery(request).await {
        Ok(exchange) => Ok(exchange),
        Err(failure) => crate::session::fail_exchange(failure),
    }
}
