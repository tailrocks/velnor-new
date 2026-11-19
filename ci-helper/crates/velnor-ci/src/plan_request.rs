// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! `velnor-ci plan --mode <ci|qualification> --request FILE`: the plan contract of the
//! legacy `plan-v1` op, for the runs that need no diff, from the request `write-request`
//! writes. It reads no cargo metadata and runs neither cargo nor mise.
//!
//! What it writes are the step outputs the generated workflows read: `plan_id`, `run_key`,
//! `not_affected_jobs` and the `qualification_*` values that need no task digest. The
//! task graph, `plan.json`, `matrix.json` and `qualification_cache_directives` carry task
//! digests the engine computes from a closure and a tool catalog; they stay with the
//! engine and are not written here.

mod qualification;

use serde::Deserialize;
use serde_json::Value;

use crate::error::CiError;
use crate::write_request::internal;
use qualification::Dispatch;

const SCHEMA: u32 = 1;
const OP: &str = "plan-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Ci,
    Qualification,
}

/// The values the run key comes from when the request carries none.
#[derive(Debug, Clone, Copy, Default)]
pub struct RunKeySource<'a> {
    pub run_id: Option<&'a str>,
    pub attempt_override: Option<&'a str>,
    pub run_attempt: Option<&'a str>,
}

/// The fields the legacy request has; any other key is refused, as the legacy op does.
const FIELDS: [&str; 11] = [
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

/// The fields the contract outputs depend on; `base`, `root`, the lane inventory and the
/// named check lanes only matter to the task graph, which this mode does not build.
#[derive(Debug, Deserialize)]
struct Request {
    schema: u32,
    #[serde(default)]
    op: Option<String>,
    #[serde(default)]
    run_key: String,
    head: String,
    event: Event,
    #[serde(default)]
    qualification: Option<Dispatch>,
    #[serde(default)]
    repository: Option<String>,
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

/// The step outputs for a request, in the order the legacy op writes them.
pub fn outputs(
    request_json: &str,
    mode: Mode,
    default_branch: Option<&str>,
    source: &RunKeySource<'_>,
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
        context.require_no_predecessor()?;
        context.bind_run(&run_key)?;
    }
    check_context(&request)?;
    if matches!(
        request.event,
        Event::PullRequest | Event::Fork | Event::Local
    ) {
        return Err(internal(
            "diff_selection_not_supported: only push, merge_group and qualification runs select every job",
        ));
    }
    let context = request.qualification.as_ref();
    let line = |name: &str, value: &str| format!("{name}={value}\n");
    Ok([
        line("plan_id", &format!("plan-{run_key}")),
        line("run_key", &run_key),
        line("not_affected_jobs", ""),
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

fn flag(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

fn parse(text: &str) -> Result<Request, CiError> {
    let value: Value = serde_json::from_str(text)
        .map_err(|error| internal(&format!("malformed_request:{error}")))?;
    if let Some(unknown) = value
        .as_object()
        .and_then(|fields| fields.keys().find(|key| !FIELDS.contains(&key.as_str())))
    {
        return Err(internal(&format!("malformed_request:unknown field `{unknown}`")));
    }
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
        (Mode::Ci, Event::Qualification) => Err(internal("qualification_event_needs_qualification_mode")),
        (Mode::Qualification, _) => Err(internal("qualification_mode_needs_qualification_event")),
        (Mode::Ci, _) => Ok(()),
    }
}

fn check_context(request: &Request) -> Result<(), CiError> {
    match (request.event, request.qualification.as_ref()) {
        (Event::Qualification, Some(_)) => Ok(()),
        (Event::Qualification, None) => Err(internal("missing_qualification_context")),
        (_, None) => Ok(()),
        (_, Some(_)) => Err(internal("qualification_context_on_wrong_event")),
    }
}

fn run_key(explicit: &str, source: &RunKeySource<'_>) -> Result<String, CiError> {
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
