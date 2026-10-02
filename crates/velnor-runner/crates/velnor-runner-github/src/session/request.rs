//! One call. The path has no host. `Debug` redacts bodies.

use std::fmt;

use zeroize::Zeroize;

use crate::TransportFail;

/// Verb used by the pinned session client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum Method {
    /// `GET` the message queue.
    Get,
    /// `POST` acquire or JIT.
    Post,
    /// `DELETE` a message or the session.
    Delete,
}

/// Outbound call. The body is wiped on drop and hidden from [`Debug`].
#[derive(Clone, PartialEq, Eq)]
#[must_use]
pub struct SessionRequest {
    /// Verb.
    pub method: Method,
    /// Path relative to the service. No scheme and no host.
    pub path: String,
    /// Raw query without `?`. `None` when the call has no query.
    pub query: Option<String>,
    /// Header pairs. Polls include [`crate::CAPACITY_HEADER`].
    pub headers: Vec<(String, String)>,
    /// Request bytes. [`Debug`] prints `[redacted]` instead.
    pub body: Vec<u8>,
}

impl fmt::Debug for SessionRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SessionRequest")
            .field("method", &self.method)
            .field("path", &self.path)
            .field("query", &self.query)
            .field("headers", &self.headers)
            .field("body", &"[redacted]")
            .finish()
    }
}

impl Drop for SessionRequest {
    fn drop(&mut self) {
        self.body.zeroize();
    }
}

/// Completed HTTP exchange. [`Debug`] hides the body.
#[derive(Clone, PartialEq, Eq)]
#[must_use]
pub struct Exchange {
    /// Status code.
    pub status: u16,
    /// Response bytes. [`Debug`] prints `[redacted]` instead.
    pub body: Vec<u8>,
}

impl fmt::Debug for Exchange {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Exchange")
            .field("status", &self.status)
            .field("body", &"[redacted]")
            .finish()
    }
}

impl Drop for Exchange {
    fn drop(&mut self) {
        self.body.zeroize();
    }
}

/// Injected exchange. Implementations must not log request or response bodies.
pub trait Transport {
    /// Send one request.
    ///
    /// # Errors
    ///
    /// [`TransportFail::Timeout`] and [`TransportFail::Reset`] leave the effect
    /// unknown. [`TransportFail::Http`] is a completed status with an empty body.
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail>;
}
