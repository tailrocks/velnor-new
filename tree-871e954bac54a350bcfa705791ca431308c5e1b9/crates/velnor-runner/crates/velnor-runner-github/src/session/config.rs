//! `POST .../generatejitconfig`. The request JSON and the response are not logged.

use crate::refresh::{StatusClass, classify_status};
use crate::{EncodedJit, WireError, jit_path};

use super::error::{SessionError, reject};
use super::request::{Method, SessionRequest, Transport};
use super::retry::{API_QUERY, execute, fresh_gate, json_content};

/// `POST` `request_json` to the JIT route. The response body is [`EncodedJit`].
///
/// Neither the request bytes nor the response are written into [`std::fmt::Debug`]
/// or [`std::fmt::Display`]. This call does not refresh on HTTP 401.
///
/// # Errors
///
/// Returns [`SessionError::Uncertain`] on timeout or reset,
/// [`SessionError::Wire`] when the status is not 200 or the body is not UTF-8,
/// and [`SessionError::Conflict`] on HTTP 409.
pub fn jit<T>(
    transport: &mut T,
    scale_set_id: i64,
    request_json: &[u8],
) -> Result<EncodedJit, SessionError>
where
    T: Transport + ?Sized,
{
    let request = SessionRequest {
        method: Method::Post,
        path: jit_path(scale_set_id),
        query: Some(API_QUERY.to_owned()),
        headers: vec![json_content()],
        body: request_json.to_vec(),
    };
    let exchange = execute(transport, &request)?;
    require_ok(exchange.status)?;
    let text = std::str::from_utf8(&exchange.body).map_err(|_| WireError::Malformed)?;
    Ok(EncodedJit::new(text.to_owned()))
}

fn require_ok(status: u16) -> Result<(), SessionError> {
    match classify_status(status, &fresh_gate()) {
        Ok(StatusClass::Ok) => Ok(()),
        Ok(class) => Err(reject(class)),
        Err(error) => Err(SessionError::from(error)),
    }
}
