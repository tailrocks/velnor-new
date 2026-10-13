//! Runner lookup and removal through the pinned distributed-task API.

use serde::Deserialize;

use crate::paths::{RUNNER_ENDPOINT, query_escape, runner_path};
use crate::session::{API_QUERY, bearer, execute, json_content, user_agent};
use crate::{Method, RunnerReference, SessionError, SessionRequest, Transport, WireError};

/// Find a runner by its exact name. Only an HTTP 200 response with `count: 0`
/// means absence.
///
/// The `agentName` query value is encoded like Go `url.Values.Encode`.
///
/// # Errors
///
/// Returns [`SessionError::Uncertain`] on timeout or reset. HTTP 404 and every
/// other non-200 status are definite errors. A count greater than one returns
/// [`WireError::MultipleResults`]; a malformed list returns
/// [`WireError::Malformed`].
pub fn get_runner_by_name<T>(
    transport: &mut T,
    runner_name: &str,
    admin_token: &str,
) -> Result<Option<RunnerReference>, SessionError>
where
    T: Transport + ?Sized,
{
    let request = SessionRequest {
        method: Method::Get,
        path: RUNNER_ENDPOINT.to_owned(),
        query: Some(format!(
            "agentName={}&{API_QUERY}",
            query_escape(runner_name)
        )),
        headers: admin_headers(admin_token)?,
        body: Vec::new(),
    };
    let exchange = execute(transport, &request)?;
    if exchange.status != 200 {
        return Err(runner_status_error(exchange.status));
    }
    decode_runner_page(&exchange.body)
}

/// Remove a runner by its service id. HTTP 204 is the only success status.
///
/// # Errors
///
/// Returns [`SessionError::Uncertain`] on timeout or reset. Every status other
/// than 204 is a definite error.
pub fn remove_runner<T>(
    transport: &mut T,
    runner_id: i64,
    admin_token: &str,
) -> Result<(), SessionError>
where
    T: Transport + ?Sized,
{
    let request = SessionRequest {
        method: Method::Delete,
        path: runner_path(runner_id),
        query: Some(API_QUERY.to_owned()),
        headers: admin_headers(admin_token)?,
        body: Vec::new(),
    };
    let exchange = execute(transport, &request)?;
    if exchange.status == 204 {
        Ok(())
    } else {
        Err(runner_status_error(exchange.status))
    }
}

fn admin_headers(admin_token: &str) -> Result<Vec<(String, String)>, SessionError> {
    Ok(vec![json_content(), bearer(admin_token)?, user_agent()])
}

fn runner_status_error(status: u16) -> SessionError {
    if status == 403 {
        SessionError::Wire(WireError::Forbidden)
    } else {
        SessionError::Wire(WireError::UnexpectedStatus)
    }
}

fn decode_runner_page(body: &[u8]) -> Result<Option<RunnerReference>, SessionError> {
    let page: RunnerReferencePage =
        serde_json::from_slice(body).map_err(|_| WireError::Malformed)?;
    match page.count {
        0 if page.value.is_empty() => Ok(None),
        0 => Err(SessionError::Wire(WireError::Malformed)),
        1 if page.value.len() == 1 => page
            .value
            .into_iter()
            .next()
            .map(Some)
            .ok_or(WireError::Malformed.into()),
        count if count > 1 => Err(SessionError::Wire(WireError::MultipleResults)),
        _ => Err(SessionError::Wire(WireError::Malformed)),
    }
}

#[derive(Deserialize)]
struct RunnerReferencePage {
    count: i64,
    #[serde(default)]
    value: Vec<RunnerReference>,
}
