//! One refresh, then the same request. HTTP 403 does not loop.

use std::mem;

use zeroize::Zeroize;

use crate::paths::{CAPACITY_HEADER, capacity_header_value, last_message_query};
use crate::refresh::{RefreshGate, StatusClass, classify_status};
use crate::{Certainty, TransportFail, WireError};

use super::error::SessionError;
use super::request::{Exchange, SessionRequest, Transport};

pub(crate) const API_QUERY: &str = "api-version=6.0-preview";

pub(crate) const fn fresh_gate() -> RefreshGate {
    RefreshGate::new()
}

pub(crate) struct Answer {
    pub(crate) class: StatusClass,
    pub(crate) status: u16,
    body: Vec<u8>,
}

impl Answer {
    pub(crate) const fn body(&self) -> &[u8] {
        self.body.as_slice()
    }
}

impl Drop for Answer {
    fn drop(&mut self) {
        self.body.zeroize();
    }
}

pub(crate) fn json_content() -> (String, String) {
    ("Content-Type".to_owned(), "application/json".to_owned())
}

pub(crate) fn accept_preview() -> (String, String) {
    (
        "Accept".to_owned(),
        "application/json; api-version=6.0-preview".to_owned(),
    )
}

pub(crate) fn capacity_pair(total: u32) -> (String, String) {
    (CAPACITY_HEADER.to_owned(), capacity_header_value(total))
}

/// Product token. Not the GitHub Actions runner user agent.
pub(crate) const USER_AGENT: &str = "velnor-host";

pub(crate) fn user_agent() -> (String, String) {
    ("User-Agent".to_owned(), USER_AGENT.to_owned())
}

/// Queue or admin bearer. An empty token never reaches the transport.
///
/// # Errors
///
/// Returns [`WireError::RegistrationRejected`] when `token` is empty.
pub(crate) fn bearer(token: &str) -> Result<(String, String), SessionError> {
    if token.is_empty() {
        return Err(SessionError::Wire(WireError::RegistrationRejected));
    }
    Ok(("Authorization".to_owned(), format!("Bearer {token}")))
}

pub(crate) fn poll_query(cursor: i64) -> String {
    match last_message_query(cursor) {
        Some(last) => format!("{API_QUERY}&{last}"),
        None => API_QUERY.to_owned(),
    }
}

pub(crate) fn execute<T>(
    transport: &mut T,
    request: &SessionRequest,
) -> Result<Exchange, SessionError>
where
    T: Transport + ?Sized,
{
    match transport.exchange(request) {
        Ok(exchange) => Ok(exchange),
        Err(fail) => fail_exchange(fail),
    }
}

pub(crate) fn fail_exchange(fail: TransportFail) -> Result<Exchange, SessionError> {
    if effect_is_uncertain(fail) {
        return Err(SessionError::Uncertain);
    }
    let TransportFail::Http(status) = fail else {
        return Err(SessionError::Uncertain);
    };
    Ok(Exchange {
        status,
        body: Vec::new(),
    })
}

fn effect_is_uncertain(fail: TransportFail) -> bool {
    crate::effect_certainty(fail) == Certainty::Uncertain
}

pub(crate) fn attempt<T, R>(
    transport: &mut T,
    request: &mut SessionRequest,
    gate: &RefreshGate,
    refresh: R,
) -> Result<Answer, SessionError>
where
    T: Transport + ?Sized,
    R: FnMut(&mut T, &mut SessionRequest) -> Result<(), SessionError>,
{
    let mut exchange = execute(transport, request)?;
    if exchange.status >= 500 {
        return Err(SessionError::Uncertain);
    }
    match classify_status(exchange.status, gate) {
        Ok(StatusClass::RefreshOnce) => refresh_once(transport, request, gate, refresh),
        Ok(class) => Ok(taken(class, &mut exchange)),
        Err(error) => Err(SessionError::Wire(error)),
    }
}

fn refresh_once<T, R>(
    transport: &mut T,
    request: &mut SessionRequest,
    gate: &RefreshGate,
    mut refresh: R,
) -> Result<Answer, SessionError>
where
    T: Transport + ?Sized,
    R: FnMut(&mut T, &mut SessionRequest) -> Result<(), SessionError>,
{
    refresh(transport, request)?;
    finished(transport, request, gate)
}

fn finished<T>(
    transport: &mut T,
    request: &mut SessionRequest,
    gate: &RefreshGate,
) -> Result<Answer, SessionError>
where
    T: Transport + ?Sized,
{
    let mut exchange = execute(transport, request)?;
    if exchange.status >= 500 {
        return Err(SessionError::Uncertain);
    }
    match classify_status(exchange.status, gate) {
        Ok(StatusClass::RefreshOnce) => Err(SessionError::Wire(WireError::RefreshExhausted)),
        Ok(class) => Ok(taken(class, &mut exchange)),
        Err(error) => Err(SessionError::Wire(error)),
    }
}

fn taken(class: StatusClass, exchange: &mut Exchange) -> Answer {
    Answer {
        class,
        status: exchange.status,
        body: mem::take(&mut exchange.body),
    }
}
