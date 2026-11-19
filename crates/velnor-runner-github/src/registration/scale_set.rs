// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

use serde::Deserialize;

use crate::paths::query_escape;
use crate::session::{API_QUERY, bearer, execute, json_content, user_agent};
use crate::{Method, SessionError, SessionRequest, Transport, WireError, scale_set_path};

use super::{CreateLabel, ScaleSetView, accept_scale_set_labels, other_status, outgoing_json};

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

#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub enum ScaleSetFound {
    NotFound,

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
    decode_page(&exchange.body, call.name, call.expected_labels)
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
    decode_one(&exchange.body, call.name, call.expected_labels)
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
    decode_one(&exchange.body, call.name, &expected_labels)
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
            Ok(ScaleSetFound::Found(view))
        }
        _ => Err(SessionError::from(WireError::RegistrationRejected)),
    }
}

fn decode_one(
    body: &[u8],
    expected_name: &str,
    expected_labels: &[String],
) -> Result<ScaleSetView, SessionError> {
    let view: ScaleSetView = serde_json::from_slice(body).map_err(|_| WireError::Malformed)?;
    accept_scale_set_labels(&view, expected_name, expected_labels)?;
    Ok(view)
}

#[derive(Deserialize)]
struct ScaleSetPage {
    count: i64,
    #[serde(default)]
    value: Vec<ScaleSetView>,
}
