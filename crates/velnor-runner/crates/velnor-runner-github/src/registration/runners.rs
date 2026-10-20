//! Runner lookup and removal through the pinned distributed-task API.

mod absence;
pub use absence::{
    RunnerAbsenceReceipt, RunnerObservationOperation, RunnerRemovalStatus, RunnerResponseReceipt,
    ensure_runner_absent,
};

use serde::Deserialize;

use crate::paths::{RUNNER_ENDPOINT, query_escape, runner_path};
use crate::session::{API_QUERY, bearer, execute, json_content, user_agent};
use crate::{Method, RunnerReference, SessionError, SessionRequest, Transport, WireError};

/// Find one runner by its exact name. Only a valid HTTP 200 response with
/// `count: 0` and an empty `value` means absence.
///
/// # Errors
///
/// Returns [`SessionError::Uncertain`] on timeout or reset. Every non-200
/// status is an error. Multiple or malformed results are rejected.
pub fn get_runner_by_name<T>(
    transport: &mut T,
    runner_name: &str,
    admin_token: &str,
) -> Result<Option<RunnerReference>, SessionError>
where
    T: Transport + ?Sized,
{
    let (_request, exchange) = lookup_runner_response(transport, runner_name, admin_token)?;
    if exchange.status != 200 {
        return Err(runner_status_error(exchange.status));
    }
    decode_runner_page(&exchange.body, runner_name).map(|(runner, _, _)| runner)
}

pub(super) fn lookup_runner_with_receipt<T>(
    transport: &mut T,
    runner_name: &str,
    admin_token: &str,
) -> Result<(Option<RunnerReference>, RunnerResponseReceipt), SessionError>
where
    T: Transport + ?Sized,
{
    let (request, exchange) = lookup_runner_response(transport, runner_name, admin_token)?;
    if exchange.status != 200 {
        return Err(runner_status_error(exchange.status));
    }
    let (runner, count, value_count) = decode_runner_page(&exchange.body, runner_name)?;
    let receipt = RunnerResponseReceipt::verified(
        RunnerObservationOperation::LookupByName,
        &request,
        exchange.status,
        Some(count),
        Some(value_count),
        runner.as_ref(),
    )?;
    Ok((runner, receipt))
}

fn lookup_runner_response<T>(
    transport: &mut T,
    runner_name: &str,
    admin_token: &str,
) -> Result<(SessionRequest, crate::Exchange), SessionError>
where
    T: Transport + ?Sized,
{
    if runner_name.is_empty() {
        return Err(WireError::RegistrationRejected.into());
    }
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
    Ok((request, exchange))
}

/// Remove a runner by its positive service id. HTTP 204 is the only success.
///
/// # Errors
///
/// Returns [`SessionError::Uncertain`] on timeout or reset. Every status other
/// than 204 is an error.
pub fn remove_runner<T>(
    transport: &mut T,
    runner_id: i64,
    admin_token: &str,
) -> Result<(), SessionError>
where
    T: Transport + ?Sized,
{
    let request = runner_delete_request(runner_id, admin_token)?;
    let exchange = execute(transport, &request)?;
    if exchange.status == 204 {
        Ok(())
    } else {
        Err(runner_status_error(exchange.status))
    }
}

pub(super) fn remove_runner_exchange<T>(
    transport: &mut T,
    runner_id: i64,
    admin_token: &str,
) -> (RunnerResponseReceipt, Result<crate::Exchange, SessionError>)
where
    T: Transport + ?Sized,
{
    let request = match runner_delete_request(runner_id, admin_token) {
        Ok(request) => request,
        Err(error) => {
            return (
                RunnerResponseReceipt::empty(RunnerObservationOperation::RemoveById),
                Err(error),
            );
        }
    };
    let (receipt, exchange) =
        absence::exchange_receipt(transport, &request, RunnerObservationOperation::RemoveById);
    (receipt, exchange)
}

fn runner_delete_request(
    runner_id: i64,
    admin_token: &str,
) -> Result<SessionRequest, SessionError> {
    if runner_id <= 0 {
        return Err(WireError::RegistrationRejected.into());
    }
    Ok(SessionRequest {
        method: Method::Delete,
        path: runner_path(runner_id),
        query: Some(API_QUERY.to_owned()),
        headers: admin_headers(admin_token)?,
        body: Vec::new(),
    })
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

fn decode_runner_page(
    body: &[u8],
    expected_name: &str,
) -> Result<(Option<RunnerReference>, i64, usize), SessionError> {
    let page: RunnerReferencePage =
        serde_json::from_slice(body).map_err(|_| WireError::Malformed)?;
    match page.count {
        0 if page.value.is_empty() => Ok((None, page.count, page.value.len())),
        0 => Err(WireError::Malformed.into()),
        1 if page.value.len() == 1 => {
            let value_count = page.value.len();
            let runner = page.value.into_iter().next().ok_or(WireError::Malformed)?;
            if runner.id <= 0
                || runner.runner_scale_set_id <= 0
                || runner.name.is_empty()
                || runner.name != expected_name
            {
                return Err(WireError::Malformed.into());
            }
            Ok((Some(runner), page.count, value_count))
        }
        count if count > 1 => Err(WireError::MultipleResults.into()),
        _ => Err(WireError::Malformed.into()),
    }
}

#[derive(Deserialize)]
struct RunnerReferencePage {
    count: i64,
    value: Vec<RunnerReference>,
}
