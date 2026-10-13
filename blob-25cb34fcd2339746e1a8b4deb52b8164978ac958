//! Get and create `_apis/runtime/runnerscalesets`. No capacity header.

use std::fmt;

use serde::Deserialize;

use crate::paths::query_escape;
use crate::session::{API_QUERY, bearer, execute, json_content, user_agent};
use crate::{Method, SessionError, SessionRequest, Transport, WireError, scale_set_path};

use super::{CreateLabel, ScaleSetView, accept_scale_set, other_status, outgoing_json};

/// Get-by-name inputs. `Debug` hides `admin_token`.
#[derive(Clone, Copy, PartialEq, Eq)]
#[must_use]
pub struct ScaleSetByName<'a> {
    /// `runnerGroupId` query value.
    pub runner_group_id: i64,
    /// Expected scale-set name.
    pub name: &'a str,
    /// Actions admin bearer.
    pub admin_token: &'a str,
}

impl fmt::Debug for ScaleSetByName<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ScaleSetByName")
            .field("runner_group_id", &self.runner_group_id)
            .field("name", &self.name)
            .field("admin_token", &"[redacted]")
            .finish()
    }
}

/// Get-by-id inputs. `Debug` hides `admin_token`.
#[derive(Clone, Copy, PartialEq, Eq)]
#[must_use]
pub struct ScaleSetById<'a> {
    /// Scale-set id placed on the path.
    pub scale_set_id: i64,
    /// Name [`accept_scale_set`](crate::accept_scale_set) must see.
    pub name: &'a str,
    /// Actions admin bearer.
    pub admin_token: &'a str,
}

impl fmt::Debug for ScaleSetById<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ScaleSetById")
            .field("scale_set_id", &self.scale_set_id)
            .field("name", &self.name)
            .field("admin_token", &"[redacted]")
            .finish()
    }
}

/// Create inputs. `Debug` hides `admin_token`.
#[derive(Clone, Copy, PartialEq, Eq)]
#[must_use]
pub struct ScaleSetCreate<'a> {
    /// Scale-set name.
    pub name: &'a str,
    /// `runnerGroupId`. Zero is omitted, matching Go `omitempty`.
    pub runner_group_id: i64,
    /// Labels before default types. Empty gets one `System` label named `name`.
    pub labels: &'a [CreateLabel],
    /// Actions admin bearer.
    pub admin_token: &'a str,
}

impl fmt::Debug for ScaleSetCreate<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ScaleSetCreate")
            .field("name", &self.name)
            .field("runner_group_id", &self.runner_group_id)
            .field("labels", &self.labels)
            .field("admin_token", &"[redacted]")
            .finish()
    }
}

/// Get-by-name outcome. `count == 0` is not an error.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub enum ScaleSetFound {
    /// The service returned `count` 0. The caller may create.
    NotFound,
    /// One set passed [`accept_scale_set`](crate::accept_scale_set).
    Found(ScaleSetView),
}

/// `GET` the collection filtered by group and name.
///
/// The query is sorted like Go `url.Values.Encode` and always includes
/// `api-version=6.0-preview`. Unreserved bytes stay as written, so
/// `ubuntu-26.04-scale-set` is not percent-encoded. Space becomes `+`.
/// Other bytes become uppercase `%XX`. No `X-ScaleSetMaxCapacity` header.
/// `count` must equal `value.len()`. This call does not refresh.
///
/// # Errors
///
/// Returns [`WireError::RegistrationRejected`] for an empty admin token, for
/// `count > 1`, or when [`accept_scale_set`](crate::accept_scale_set) fails.
/// A count that does not match `value` is [`WireError::Malformed`].
/// Any status other than 200 is the pinned status class.
pub fn get_runner_scale_set<T>(
    transport: &mut T,
    call: &ScaleSetByName<'_>,
) -> Result<ScaleSetFound, SessionError>
where
    T: Transport + ?Sized,
{
    let request = SessionRequest {
        method: Method::Get,
        path: scale_set_path().to_owned(),
        query: Some(name_query(call.runner_group_id, call.name)),
        headers: admin_headers(call.admin_token)?,
        body: Vec::new(),
    };
    let exchange = execute(transport, &request)?;
    if exchange.status != 200 {
        return Err(other_status(exchange.status));
    }
    decode_page(&exchange.body, call.name)
}

/// `GET .../runnerscalesets/{id}?api-version=6.0-preview`, then adopt checks.
///
/// # Errors
///
/// Returns [`WireError::RegistrationRejected`] for an empty admin token or a
/// failed adopt check. A non-200 status is the pinned status class.
/// A body that is not one scale set is [`WireError::Malformed`].
pub fn get_runner_scale_set_by_id<T>(
    transport: &mut T,
    call: &ScaleSetById<'_>,
) -> Result<ScaleSetView, SessionError>
where
    T: Transport + ?Sized,
{
    let request = SessionRequest {
        method: Method::Get,
        path: format!("{}/{}", scale_set_path(), call.scale_set_id),
        query: Some(API_QUERY.to_owned()),
        headers: admin_headers(call.admin_token)?,
        body: Vec::new(),
    };
    let exchange = execute(transport, &request)?;
    if exchange.status != 200 {
        return Err(other_status(exchange.status));
    }
    decode_one(&exchange.body, call.name)
}

/// `POST` a scale set. Empty labels become one `System` label named `name`
/// before the call, matching `ensureLabels`. Empty label types become `System`.
/// Product callers pass [`product_create_labels`](crate::product_create_labels).
///
/// # Errors
///
/// Returns [`WireError::RegistrationRejected`] for an empty admin token, or
/// when the name and the labels are both empty. Any status other than 200 is
/// [`WireError::UnexpectedStatus`]. The 200 body must pass
/// [`accept_scale_set`](crate::accept_scale_set).
pub fn create_runner_scale_set<T>(
    transport: &mut T,
    call: &ScaleSetCreate<'_>,
) -> Result<ScaleSetView, SessionError>
where
    T: Transport + ?Sized,
{
    let body = outgoing_json(call.name, call.labels, call.runner_group_id)?.into_bytes();
    let request = SessionRequest {
        method: Method::Post,
        path: scale_set_path().to_owned(),
        query: Some(API_QUERY.to_owned()),
        headers: admin_headers(call.admin_token)?,
        body,
    };
    let exchange = execute(transport, &request)?;
    if exchange.status != 200 {
        return Err(SessionError::Wire(WireError::UnexpectedStatus));
    }
    decode_one(&exchange.body, call.name)
}

fn admin_headers(admin_token: &str) -> Result<Vec<(String, String)>, SessionError> {
    Ok(vec![json_content(), bearer(admin_token)?, user_agent()])
}

fn name_query(runner_group_id: i64, name: &str) -> String {
    let group = runner_group_id.to_string();
    format!(
        "api-version=6.0-preview&name={}&runnerGroupId={}",
        query_escape(name),
        query_escape(&group),
    )
}

fn decode_page(body: &[u8], expected_name: &str) -> Result<ScaleSetFound, SessionError> {
    let page: ScaleSetPage = serde_json::from_slice(body).map_err(|_| WireError::Malformed)?;
    let len = i64::try_from(page.value.len()).map_err(|_| WireError::Malformed)?;
    if page.count != len {
        return Err(SessionError::from(WireError::Malformed));
    }
    let mut sets = page.value;
    match sets.len() {
        0 => Ok(ScaleSetFound::NotFound),
        1 => {
            let view = sets.pop().ok_or(WireError::Malformed)?;
            accept_scale_set(&view, expected_name)?;
            Ok(ScaleSetFound::Found(view))
        }
        _ => Err(SessionError::from(WireError::RegistrationRejected)),
    }
}

fn decode_one(body: &[u8], expected_name: &str) -> Result<ScaleSetView, SessionError> {
    let view: ScaleSetView = serde_json::from_slice(body).map_err(|_| WireError::Malformed)?;
    accept_scale_set(&view, expected_name)?;
    Ok(view)
}

#[derive(Deserialize)]
struct ScaleSetPage {
    count: i64,
    #[serde(default)]
    value: Vec<ScaleSetView>,
}
