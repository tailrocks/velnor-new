//! One call. The path has no host. `Debug` redacts bodies.

use std::fmt;

use zeroize::Zeroize;

use crate::{TransportFail, WireError};

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
    /// `PATCH` refreshes the message session.
    Patch,
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

impl SessionRequest {
    /// Return the token from exactly one well-formed bearer header.
    pub(crate) fn bearer_token(&self) -> Option<&str> {
        let mut authorization = self
            .headers
            .iter()
            .filter(|(key, _)| key.eq_ignore_ascii_case("authorization"));
        let (_, value) = authorization.next()?;
        if authorization.next().is_some() {
            return None;
        }
        let token = value.strip_prefix("Bearer ")?;
        if token.is_empty()
            || token
                .bytes()
                .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
        {
            return None;
        }
        Some(token)
    }

    /// Whether the request has exactly one bearer header with `token`.
    pub(crate) fn uses_bearer(&self, token: &str) -> bool {
        self.bearer_token() == Some(token)
    }

    /// Replace the request's one bearer header, wiping its previous value.
    ///
    /// # Errors
    ///
    /// Returns [`WireError::RegistrationRejected`] for an empty token or a
    /// request without exactly one authorization header.
    pub(crate) fn replace_bearer(&mut self, token: &str) -> Result<(), WireError> {
        if token.is_empty()
            || token
                .bytes()
                .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
        {
            return Err(WireError::RegistrationRejected);
        }
        let authorization_count = self
            .headers
            .iter()
            .filter(|(key, _)| key.eq_ignore_ascii_case("authorization"))
            .count();
        if authorization_count != 1 {
            return Err(WireError::RegistrationRejected);
        }
        let Some((_, value)) = self
            .headers
            .iter_mut()
            .find(|(key, _)| key.eq_ignore_ascii_case("authorization"))
        else {
            return Err(WireError::RegistrationRejected);
        };
        value.zeroize();
        *value = format!("Bearer {token}");
        Ok(())
    }
}

impl fmt::Debug for SessionRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SessionRequest")
            .field("method", &self.method)
            .field("path", &self.path)
            .field("query", &self.query)
            .field("headers", &redacted_headers(&self.headers))
            .field("body", &"[redacted]")
            .finish()
    }
}

fn redacted_headers(headers: &[(String, String)]) -> Vec<(&str, &str)> {
    headers
        .iter()
        .map(|(key, value)| {
            if key.eq_ignore_ascii_case("authorization") {
                (key.as_str(), "[redacted]")
            } else {
                (key.as_str(), value.as_str())
            }
        })
        .collect()
}

impl Drop for SessionRequest {
    fn drop(&mut self) {
        self.body.zeroize();
        for (key, value) in &mut self.headers {
            if key.eq_ignore_ascii_case("authorization") {
                value.zeroize();
            }
        }
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
