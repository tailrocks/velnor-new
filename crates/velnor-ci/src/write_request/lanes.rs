// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::internal;
use crate::error::CiError;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Variant {
    Hosted,
    ScaleSet,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Mode {
    Hosted,
    ScaleSet,
    Both,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NamedCheckLane {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    variant: Option<Variant>,
    job_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Route {
    logical_job_id: String,
    job_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    variant: Option<Variant>,
    runner: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    target: Option<String>,
    task_ids: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Inventory {
    mode: Option<Mode>,
    routes: Vec<Route>,
}

pub(super) fn named_check_lanes(text: &str) -> Result<Value, CiError> {
    let lanes: BTreeMap<String, Vec<NamedCheckLane>> =
        serde_json::from_str(text).map_err(|_| internal("malformed_named_check_lanes"))?;
    serde_json::to_value(lanes).map_err(|_| internal("malformed_named_check_lanes"))
}

pub(super) fn lane_inventory(text: &str) -> Result<Value, CiError> {
    let inventory: Inventory =
        serde_json::from_str(text).map_err(|_| internal("malformed_lane_inventory"))?;
    serde_json::to_value(inventory).map_err(|_| internal("malformed_lane_inventory"))
}

pub(crate) fn check_request_fields(request: &Value) -> Result<(), serde_json::Error> {
    if let Some(named) = request
        .get("named_check_lanes")
        .filter(|value| !value.is_null())
    {
        serde_json::from_value::<BTreeMap<String, Vec<NamedCheckLane>>>(named.clone())?;
    }
    if let Some(inventory) = request
        .get("lane_inventory")
        .filter(|value| !value.is_null())
    {
        serde_json::from_value::<Inventory>(inventory.clone())?;
    }
    Ok(())
}
