// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod diff;
mod qualification;

use std::path::Path;

use serde::Deserialize;

use crate::error::CiError;
use crate::resolve_qualification::strict;
use crate::write_request::{internal, lanes};
use qualification::Dispatch;

const SCHEMA: u32 = 1;
const OP: &str = "plan-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Ci,
    Qualification,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RunSource<'a> {
    pub github_sha: Option<&'a str>,
    pub run_id: Option<&'a str>,
    pub attempt_override: Option<&'a str>,
    pub run_attempt: Option<&'a str>,
}

const FIELDS: &[&str] = &[
    "schema",
    "op",
    "run_key",
    "base",
    "head",
    "event",
    "qualification",
    "root",
    "repository",
    "named_check_lanes",
    "lane_inventory",
];

#[derive(Debug, Deserialize)]
struct Request {
    schema: u32,
    #[serde(default)]
    op: Option<String>,
    #[serde(default)]
    run_key: String,
    #[serde(default)]
    base: Option<String>,
    #[serde(rename = "root", default)]
    _root: Option<String>,
    head: String,
    event: Event,
    #[serde(default)]
    qualification: Option<Dispatch>,
    #[serde(default)]
    repository: Option<String>,
    #[serde(default)]
    lane_inventory: Option<diff::LaneInventory>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Event {
    PullRequest,
    Push,
    MergeGroup,
    Local,
    Fork,
    Qualification,
}

pub fn outputs(
    request_json: &str,
    mode: Mode,
    default_branch: Option<&str>,
    source: &RunSource<'_>,
    checkout: &Path,
) -> Result<String, CiError> {
    let request = parse(request_json)?;
    let run_key = run_key(&request.run_key, source)?;
    if request.head.trim().is_empty() {
        return Err(internal("empty_head"));
    }
    check_mode(mode, request.event)?;
    if let (Event::Qualification, Some(context)) = (request.event, &request.qualification) {
        let branch = default_branch.ok_or_else(|| internal("missing_default_branch"))?;
        let repository = request
            .repository
            .as_deref()
            .ok_or_else(|| internal("missing_qualification_repository"))?;
        context.validate_for(branch, repository, &request.head)?;
        context.bind_current_sha(source.github_sha)?;
        context.require_no_predecessor()?;
        context.bind_run(&run_key)?;
    }
    check_context(&request)?;
    let not_affected = match request.event {
        Event::PullRequest => selected(&request, diff::Event::PullRequest, checkout)?,
        Event::Fork => selected(&request, diff::Event::Fork, checkout)?,
        Event::Local => selected(&request, diff::Event::Local, checkout)?,
        Event::Push | Event::MergeGroup | Event::Qualification => String::new(),
    };
    let context = request.qualification.as_ref();
    let line = |name: &str, value: &str| format!("{name}={value}\n");
    Ok([
        line("plan_id", &format!("plan-{run_key}")),
        line("run_key", &run_key),
        line("not_affected_jobs", &not_affected),
        line(
            "qualification_campaign",
            context.map_or("", |value| value.campaign.as_str()),
        ),
        line(
            "qualification_phase",
            context.map_or("", |value| value.phase.as_str()),
        ),
        line(
            "qualification_cache_enabled",
            flag(context.is_some_and(|value| value.phase.cache_enabled())),
        ),
        line("qualification_cache_write", "false"),
    ]
    .concat())
}

fn selected(request: &Request, event: diff::Event, checkout: &Path) -> Result<String, CiError> {
    diff::not_affected_jobs(
        &diff::Input {
            event,
            base: request.base.as_deref(),
            head: &request.head,
            inventory: request.lane_inventory.as_ref(),
        },
        checkout,
    )
}

fn flag(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

fn parse(text: &str) -> Result<Request, CiError> {
    let value = strict::parse(text)?;
    if let Some(unknown) = value
        .as_object()
        .and_then(|fields| fields.keys().find(|key| !FIELDS.contains(&key.as_str())))
    {
        return Err(internal(&format!(
            "malformed_request:unknown field `{unknown}`"
        )));
    }
    lanes::check_request_fields(&value)
        .map_err(|error| internal(&format!("malformed_request:{error}")))?;
    let request: Request = serde_json::from_value(value)
        .map_err(|error| internal(&format!("malformed_request:{error}")))?;
    if request.schema != SCHEMA {
        return Err(internal(&format!("unsupported_schema:{}", request.schema)));
    }
    if request.op.as_deref().is_some_and(|op| op != OP) {
        return Err(internal("op_mismatch"));
    }
    Ok(request)
}

fn check_mode(mode: Mode, event: Event) -> Result<(), CiError> {
    match (mode, event) {
        (Mode::Qualification, Event::Qualification) => Ok(()),
        (Mode::Ci, Event::Qualification) => {
            Err(internal("qualification_event_needs_qualification_mode"))
        }
        (Mode::Qualification, _) => Err(internal("qualification_mode_needs_qualification_event")),
        (Mode::Ci, _) => Ok(()),
    }
}

fn check_context(request: &Request) -> Result<(), CiError> {
    match (request.event, request.qualification.as_ref()) {
        (Event::Qualification, None) => Err(internal("missing_qualification_context")),
        (Event::Qualification, Some(_)) | (_, None) => Ok(()),
        (_, Some(_)) => Err(internal("qualification_context_on_wrong_event")),
    }
}

fn run_key(explicit: &str, source: &RunSource<'_>) -> Result<String, CiError> {
    if !explicit.trim().is_empty() {
        return if is_run_key(explicit) {
            Ok(explicit.to_owned())
        } else {
            Err(internal("invalid run_key: malformed_run_key"))
        };
    }
    let id = non_empty(source.run_id);
    let attempt = non_empty(source.attempt_override).or_else(|| non_empty(source.run_attempt));
    let (Some(id), Some(attempt)) = (id, attempt) else {
        return Err(internal("missing_run_key"));
    };
    let id: u64 = id.parse().map_err(|_| internal("bad_run_id"))?;
    let attempt: u64 = attempt.parse().map_err(|_| internal("bad_run_attempt"))?;
    Ok(format!("r{id}-a{attempt}"))
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.filter(|text| !text.is_empty())
}

fn is_run_key(value: &str) -> bool {
    let digits = |text: &str| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
    value == "local"
        || value
            .strip_prefix('r')
            .and_then(|rest| rest.split_once("-a"))
            .is_some_and(|(run, attempt)| digits(run) && digits(attempt))
}
