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

/// Semantic class for an outbound request. This is routing metadata, not an
/// authorization decision; transports must still validate the exact method,
/// path, query, headers, body, and bound origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum RequestPurpose {
    /// Read the configured repository from GitHub REST.
    RepositoryRead,
    /// Read a workflow run, job, or repository Actions policy from GitHub REST.
    ActionsRead,
    /// Read private-repository fork-workflow controls from GitHub REST.
    PrivateForkPolicyRead,
    /// Issue a runner registration token through GitHub REST.
    RegistrationTokenIssue,
    /// Exchange a registration token for Actions Service credentials.
    ActionsAdminExchange,
    /// Read runner-group or Scale Set metadata from Actions Service.
    ActionsMetadataRead,
    /// Create a Scale Set through Actions Service.
    ScaleSetCreate,
    /// Read a runner through Actions Service.
    RunnerRead,
    /// Remove a runner through Actions Service.
    RunnerDelete,
    /// Create one Scale Set message session.
    SessionCreate,
    /// Refresh one existing Scale Set message session.
    SessionRefresh,
    /// Poll the session message queue.
    MessageQueuePoll,
    /// Acquire Scale Set jobs through Actions Service.
    AcquireJobs,
    /// Request a JIT configuration from Actions Service.
    GenerateJitConfig,
    /// Acknowledge one exact message on the message queue.
    MessageAcknowledge,
    /// Close one exact Scale Set session.
    SessionClose,
}

/// Credential audience placed in the request bearer header.
///
/// This marker is not itself proof that a credential is valid or authorized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum BearerRole {
    /// Caller-held GitHub REST credential.
    GithubRestCredential,
    /// Short-lived registration token used only for the admin exchange.
    RegistrationToken,
    /// Actions Service administration credential.
    ActionsAdmin,
    /// Session-specific message queue credential.
    SessionQueue,
}

/// Outbound call. The body is wiped on drop and hidden from [`Debug`].
#[derive(Clone, PartialEq, Eq)]
#[must_use]
pub struct SessionRequest {
    /// Semantic operation class (not an authorization decision).
    pub purpose: RequestPurpose,
    /// Audience of the one bearer authorization value.
    pub bearer_role: BearerRole,
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

    pub(crate) fn replace_target(&mut self, path: String, query: Option<String>) {
        self.path.zeroize();
        if let Some(value) = &mut self.query {
            value.zeroize();
        }
        self.path = path;
        self.query = query;
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
            .field("purpose", &self.purpose)
            .field("bearer_role", &self.bearer_role)
            .field("method", &self.method)
            .field("path", &"[redacted]")
            .field("query", &self.query.as_ref().map(|_| "[redacted]"))
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
        self.path.zeroize();
        if let Some(query) = &mut self.query {
            query.zeroize();
        }
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
