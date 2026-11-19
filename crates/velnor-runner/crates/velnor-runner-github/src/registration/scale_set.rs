// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

use serde::Deserialize;

use crate::paths::query_escape;
use crate::session::{API_QUERY, bearer, execute, json_content, user_agent};
use crate::{Method, SessionError, SessionRequest, Transport, WireError, scale_set_path};

use super::{
    CreateLabel, ScaleSetView, accept_scale_set_group, accept_scale_set_labels, other_status,
    outgoing_json,
};

#[derive(Clone, Copy, PartialEq, Eq)]
#[must_use]
pub struct ScaleSetByName<'a> {
    pub runner_group_id: i64,

    pub name: &'a str,

    pub expected_labels: &'a [String],

    pub admin_token: &'a str,
}

impl fmt::Debug for ScaleSetByName<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ScaleSetByName")
            .field("runner_group_id", &self.runner_group_id)
            .field("name", &self.name)
            .field("expected_labels", &self.expected_labels)
            .field("admin_token", &"[redacted]")
            .finish()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[must_use]
pub struct ScaleSetById<'a> {
    pub scale_set_id: i64,

    pub name: &'a str,

    pub expected_labels: &'a [String],

    pub admin_token: &'a str,
}

impl fmt::Debug for ScaleSetById<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ScaleSetById")
            .field("scale_set_id", &self.scale_set_id)
            .field("name", &self.name)
            .field("expected_labels", &self.expected_labels)
            .field("admin_token", &"[redacted]")
            .finish()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[must_use]
pub struct ScaleSetCreate<'a> {
    pub name: &'a str,

    pub runner_group_id: i64,

    pub labels: &'a [CreateLabel],

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

#[derive(Clone, Copy, PartialEq, Eq)]
#[must_use]
pub struct ScaleSetDelete<'a> {
    pub scale_set_id: i64,

    pub admin_token: &'a str,
}

impl fmt::Debug for ScaleSetDelete<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ScaleSetDelete")
            .field("scale_set_id", &self.scale_set_id)
            .field("admin_token", &"[redacted]")
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub enum ScaleSetFound {
    NotFound,

    Found(ScaleSetView),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum ScaleSetDeleteStatus {
    Deleted,

    NotFound,

    Refused(u16),
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub enum ScaleSetObserved {
    Missing,

    Found(ScaleSetView),
}

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
    decode_page(
        &exchange.body,
        call.runner_group_id,
        call.name,
        call.expected_labels,
    )
}

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
    decode_one(&exchange.body, None, call.name, call.expected_labels)
}

pub fn observe_runner_scale_set_by_id<T>(
    transport: &mut T,
    call: &ScaleSetById<'_>,
) -> Result<ScaleSetObserved, SessionError>
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
    match exchange.status {
        200 => {
            decode_one(&exchange.body, call.name, call.expected_labels).map(ScaleSetObserved::Found)
        }
        404 => Ok(ScaleSetObserved::Missing),
        status => Err(other_status(status)),
    }
}

pub fn delete_runner_scale_set<T>(
    transport: &mut T,
    call: &ScaleSetDelete<'_>,
) -> Result<ScaleSetDeleteStatus, SessionError>
where
    T: Transport + ?Sized,
{
    if call.scale_set_id <= 0 {
        return Err(WireError::RegistrationRejected.into());
    }
    let request = SessionRequest {
        method: Method::Delete,
        path: format!("{}/{}", scale_set_path(), call.scale_set_id),
        query: Some(API_QUERY.to_owned()),
        headers: admin_headers(call.admin_token)?,
        body: Vec::new(),
    };
    match execute(transport, &request)?.status {
        204 => Ok(ScaleSetDeleteStatus::Deleted),
        404 => Ok(ScaleSetDeleteStatus::NotFound),
        status @ (400 | 401 | 403 | 409 | 422) => Ok(ScaleSetDeleteStatus::Refused(status)),
        _ => Err(SessionError::Uncertain),
    }
}

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
    let expected_labels = if call.labels.is_empty() {
        vec![call.name.to_owned()]
    } else {
        call.labels.iter().map(|label| label.name.clone()).collect()
    };
    decode_one(
        &exchange.body,
        Some(call.runner_group_id),
        call.name,
        &expected_labels,
    )
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

fn decode_page(
    body: &[u8],
    requested_group: i64,
    expected_name: &str,
    expected_labels: &[String],
) -> Result<ScaleSetFound, SessionError> {
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
            accept_scale_set_labels(&view, expected_name, expected_labels)?;
            accept_scale_set_group(&view, requested_group)?;
            Ok(ScaleSetFound::Found(view))
        }
        _ => Err(SessionError::from(WireError::RegistrationRejected)),
    }
}

fn decode_one(
    body: &[u8],
    requested_group: Option<i64>,
    expected_name: &str,
    expected_labels: &[String],
) -> Result<ScaleSetView, SessionError> {
    let view: ScaleSetView = serde_json::from_slice(body).map_err(|_| WireError::Malformed)?;
    accept_scale_set_labels(&view, expected_name, expected_labels)?;
    if let Some(group) = requested_group {
        accept_scale_set_group(&view, group)?;
    }
    Ok(view)
}

#[derive(Deserialize)]
struct ScaleSetPage {
    count: i64,
    #[serde(default)]
    value: Vec<ScaleSetView>,
}
