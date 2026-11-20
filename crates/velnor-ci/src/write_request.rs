// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The plan request: the file `plan-v1` reads. `velnor-ci write-request` builds it
//! from the workflow run's own context, given as arguments, so no caller sets
//! `VELNOR_REQUEST_FILE`. The bytes equal what the legacy `write-request-v1` writes.

pub(crate) mod canonical;
pub(crate) mod lanes;
mod qualification;

use serde_json::{Map, Value, json};

use crate::error::CiError;

pub use qualification::{Phase, RunnerContext, dispatch_for_parts};

pub(crate) const SCHEMA: u32 = 1;
const OP: &str = "plan-v1";

/// What the run knows about itself; every field except the event name may be absent.
#[derive(Debug, Clone, Copy, Default)]
pub struct RequestParts<'a> {
    pub event_name: &'a str,
    pub payload_json: &'a str,
    pub github_sha: Option<&'a str>,
    pub repository: Option<&'a str>,
    pub runner: RunnerContext<'a>,
    pub named_check_lanes: Option<&'a str>,
    pub lane_inventory: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Event {
    PullRequest,
    Push,
    MergeGroup,
    Local,
    Fork,
    Qualification,
}

impl Event {
    const fn name(self) -> &'static str {
        match self {
            Self::PullRequest => "pull_request",
            Self::Push => "push",
            Self::MergeGroup => "merge_group",
            Self::Local => "local",
            Self::Fork => "fork",
            Self::Qualification => "qualification",
        }
    }
}

pub(crate) fn internal(problem: &str) -> CiError {
    CiError::new(format!("internal request failed: {problem}"))
}

/// The canonical JSON of the plan request, as the legacy `write-request-v1` wrote it.
pub fn build(parts: &RequestParts<'_>) -> Result<Vec<u8>, CiError> {
    let named = parts
        .named_check_lanes
        .map(lanes::named_check_lanes)
        .transpose()?;
    let inventory = parts
        .lane_inventory
        .map(lanes::lane_inventory)
        .transpose()?;
    let payload: Value = serde_json::from_str(parts.payload_json)
        .map_err(|_| internal("malformed_event_payload"))?;
    let event = event_for(parts.event_name, &payload)?;
    let qualification = dispatch_for_parts(parts.event_name, &payload, &parts.runner)?;
    let (base, head) = refs(event, &payload, parts.github_sha)?;
    let mut request = Map::new();
    request.insert("schema".to_owned(), json!(SCHEMA));
    request.insert("op".to_owned(), json!(OP));
    request.insert("event".to_owned(), json!(event.name()));
    request.insert("base".to_owned(), json!(base));
    request.insert("head".to_owned(), json!(head));
    request.insert("root".to_owned(), json!("."));
    if let Some(repository) = parts.repository.filter(|slug| !slug.is_empty()) {
        request.insert("repository".to_owned(), json!(repository));
    }
    if let Some(context) = qualification {
        request.insert("qualification".to_owned(), context);
    }
    if let Some(named) = named {
        request.insert("named_check_lanes".to_owned(), named);
    }
    if let Some(inventory) = inventory {
        request.insert("lane_inventory".to_owned(), inventory);
    }
    Ok(canonical::bytes(&Value::Object(request)))
}

fn event_for(event_name: &str, payload: &Value) -> Result<Event, CiError> {
    match event_name {
        "pull_request" => match payload["pull_request"]["head"]["repo"]["fork"].as_bool() {
            Some(true) => Ok(Event::Fork),
            Some(false) => Ok(Event::PullRequest),
            None => Err(internal("indeterminate_fork")),
        },
        "push" => Ok(Event::Push),
        "merge_group" => Ok(Event::MergeGroup),
        "workflow_dispatch" => Ok(Event::Qualification),
        "local" => Ok(Event::Local),
        _ => Err(internal("unsupported_event")),
    }
}

pub(crate) fn nonempty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

fn refs(
    event: Event,
    payload: &Value,
    github_sha: Option<&str>,
) -> Result<(Option<String>, String), CiError> {
    let need =
        |value: &Value, problem: &str| nonempty(value.as_str()).ok_or_else(|| internal(problem));
    match event {
        Event::PullRequest | Event::Fork => {
            let pr = &payload["pull_request"];
            Ok((
                Some(need(&pr["base"]["sha"], "missing_pr_base")?),
                need(&pr["head"]["sha"], "missing_pr_head")?,
            ))
        }
        Event::MergeGroup => {
            let group = &payload["merge_group"];
            Ok((
                Some(need(&group["base_sha"], "missing_merge_base")?),
                need(&group["head_sha"], "missing_merge_head")?,
            ))
        }
        Event::Push => {
            let real = |value: &Value| nonempty(value.as_str()).filter(|sha| !is_zero(sha));
            let head = real(&payload["after"])
                .or_else(|| nonempty(github_sha))
                .ok_or_else(|| internal("missing_push_head"))?;
            Ok((real(&payload["before"]), head))
        }
        Event::Qualification => Ok((
            None,
            nonempty(github_sha).ok_or_else(|| internal("missing_qualification_head"))?,
        )),
        Event::Local => Ok((
            None,
            nonempty(github_sha).unwrap_or_else(|| "HEAD".to_owned()),
        )),
    }
}

fn is_zero(sha: &str) -> bool {
    !sha.is_empty() && sha.bytes().all(|byte| byte == b'0')
}
