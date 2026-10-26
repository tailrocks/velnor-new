// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;
use serde_json::Value;

use crate::error::WireError;

pub const MAX_POLL_BODY_BYTES: usize = 256 * 1024;

pub const MAX_POLL_MESSAGES: usize = 50;
const MAX_POLL_ENVELOPE_BYTES: usize = MAX_POLL_BODY_BYTES * 8;
const MAX_JOB_ID_BYTES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Poll {
    Empty,

    Batch(ParsedBatch),

    Quarantined(QuarantinedBatch),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuarantinedBatch {
    pub message_id: i64,

    pub raw_body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InnerKind {
    Available,

    Assigned,

    Started,

    Completed,

    Unsupported(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedBatch {
    pub message_id: i64,

    pub raw_body: String,

    pub statistics: Option<Statistics>,

    pub jobs: Vec<InnerJob>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InnerJob {
    pub kind: InnerKind,

    pub request_id: Option<i64>,

    pub runner_id: Option<i64>,

    pub runner_name: Option<String>,

    pub result: Option<String>,

    pub job_id: Option<String>,

    pub labels: Vec<String>,

    pub fields: Vec<String>,

    pub source: SourceMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SourceMetadata {
    pub owner_name: Option<String>,

    pub repository_name: Option<String>,

    pub job_workflow_ref: Option<String>,

    pub workflow_run_id: Option<i64>,

    pub event_name: Option<String>,
}

impl SourceMetadata {
    #[must_use]
    pub fn complete(&self) -> Option<CompleteSourceMetadata<'_>> {
        let owner_name = non_empty(self.owner_name.as_deref())?;
        let repository_name = non_empty(self.repository_name.as_deref())?;
        let job_workflow_ref = non_empty(self.job_workflow_ref.as_deref())?;
        let workflow_run_id = self.workflow_run_id.filter(|value| *value > 0)?;
        let event_name = non_empty(self.event_name.as_deref())?;
        Some(CompleteSourceMetadata {
            owner_name,
            repository_name,
            job_workflow_ref,
            workflow_run_id,
            event_name,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompleteSourceMetadata<'a> {
    pub owner_name: &'a str,

    pub repository_name: &'a str,

    pub job_workflow_ref: &'a str,

    pub workflow_run_id: i64,

    pub event_name: &'a str,
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.filter(|text| !text.trim().is_empty())
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Statistics {
    pub total_available_jobs: i64,

    pub total_acquired_jobs: i64,

    pub total_assigned_jobs: i64,

    pub total_running_jobs: i64,

    pub total_registered_runners: i64,

    pub total_busy_runners: i64,

    pub total_idle_runners: i64,
}

impl Statistics {
    #[must_use]
    pub const fn assigned_population(&self) -> i64 {
        self.total_assigned_jobs
    }
}

#[derive(Debug, Deserialize)]
struct Envelope {
    #[serde(rename = "messageId")]
    message_id: i64,
    #[serde(rename = "messageType")]
    message_type: String,
    body: Option<String>,
    #[serde(default)]
    statistics: Option<Statistics>,
}

pub fn parse_poll(status: u16, body: &str) -> Result<Poll, WireError> {
    if status == 202 {
        return Ok(Poll::Empty);
    }
    if status != 200 {
        return Err(WireError::UnexpectedStatus);
    }
    parse_envelope(body)
}

fn parse_envelope(body: &str) -> Result<Poll, WireError> {
    if body.len() > MAX_POLL_ENVELOPE_BYTES {
        return Err(WireError::Malformed);
    }
    let envelope: Envelope = serde_json::from_str(body).map_err(|_| WireError::Malformed)?;
    if envelope.message_type != "RunnerScaleSetJobMessages" {
        return Err(WireError::UnsupportedEnvelope);
    }
    if envelope.message_id < 0 {
        return Err(WireError::Malformed);
    }
    let raw_body = envelope.body.unwrap_or_default();
    if raw_body.len() > MAX_POLL_BODY_BYTES {
        return Err(WireError::Malformed);
    }
    match parse_body(&raw_body) {
        Ok(jobs) => Ok(Poll::Batch(ParsedBatch {
            message_id: envelope.message_id,
            raw_body,
            statistics: envelope.statistics,
            jobs,
        })),
        Err(_error) => Ok(Poll::Quarantined(QuarantinedBatch {
            message_id: envelope.message_id,
            raw_body,
        })),
    }
}

fn parse_body(body: &str) -> Result<Vec<InnerJob>, WireError> {
    if body.len() > MAX_POLL_BODY_BYTES {
        return Err(WireError::Malformed);
    }
    if body.is_empty() {
        return Ok(Vec::new());
    }
    let values: Vec<Value> = serde_json::from_str(body).map_err(|_| WireError::Malformed)?;
    if values.len() > MAX_POLL_MESSAGES {
        return Err(WireError::Malformed);
    }
    values.iter().map(parse_inner).collect()
}

pub fn parse_inner_messages(body: &str) -> Result<Vec<InnerJob>, WireError> {
    parse_body(body)
}

fn parse_inner(value: &Value) -> Result<InnerJob, WireError> {
    let kind_text = value
        .get("messageType")
        .and_then(Value::as_str)
        .ok_or(WireError::Malformed)?;
    let request_id = value.get("runnerRequestId").and_then(Value::as_i64);
    let job_id = job_id_of(value);
    let labels = label_names(value.get("requestLabels"));
    let fields = object_fields(value);
    let kind = match kind_text {
        "JobAvailable" => InnerKind::Available,
        "JobAssigned" => InnerKind::Assigned,
        "JobStarted" => InnerKind::Started,
        "JobCompleted" => InnerKind::Completed,
        other => InnerKind::Unsupported(other.to_owned()),
    };
    let runner_fields = message_runner_fields(value, &kind)?;
    let source = source_metadata(value)?;
    Ok(InnerJob {
        kind,
        request_id,
        runner_id: runner_fields.runner_id,
        runner_name: runner_fields.runner_name,
        result: runner_fields.result,
        job_id,
        labels,
        fields,
        source,
    })
}

fn source_metadata(value: &Value) -> Result<SourceMetadata, WireError> {
    Ok(SourceMetadata {
        owner_name: optional_string(value, "ownerName")?,
        repository_name: optional_string(value, "repositoryName")?,
        job_workflow_ref: optional_string(value, "jobWorkflowRef")?,
        workflow_run_id: optional_i64(value, "workflowRunId")?,
        event_name: optional_string(value, "eventName")?,
    })
}

fn message_runner_fields(
    value: &Value,
    kind: &InnerKind,
) -> Result<MessageRunnerFields, WireError> {
    match kind {
        InnerKind::Started => Ok(MessageRunnerFields {
            runner_id: optional_i64(value, "runnerId")?,
            runner_name: optional_string(value, "runnerName")?,
            result: None,
        }),
        InnerKind::Completed => Ok(MessageRunnerFields {
            runner_id: optional_i64(value, "runnerId")?,
            runner_name: optional_string(value, "runnerName")?,
            result: optional_string(value, "result")?,
        }),
        InnerKind::Available | InnerKind::Assigned | InnerKind::Unsupported(_) => {
            Ok(MessageRunnerFields::default())
        }
    }
}

#[derive(Default)]
struct MessageRunnerFields {
    runner_id: Option<i64>,
    runner_name: Option<String>,
    result: Option<String>,
}

fn optional_i64(value: &Value, field: &str) -> Result<Option<i64>, WireError> {
    match value.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(number) => number.as_i64().map(Some).ok_or(WireError::Malformed),
    }
}

fn optional_string(value: &Value, field: &str) -> Result<Option<String>, WireError> {
    match value.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(text) => text
            .as_str()
            .map(str::to_owned)
            .map(Some)
            .ok_or(WireError::Malformed),
    }
}

fn job_id_of(value: &Value) -> Option<String> {
    match value.get("jobId")? {
        Value::String(text) => opaque_job_id(text),
        Value::Number(number) => positive_job_id(number.as_i64()),
        _ => None,
    }
}

fn positive_job_id(value: Option<i64>) -> Option<String> {
    let number = value.filter(|item| *item > 0)?;
    Some(number.to_string())
}

fn opaque_job_id(value: &str) -> Option<String> {
    if value.is_empty() || value.len() > MAX_JOB_ID_BYTES || value.chars().any(char::is_control) {
        return None;
    }
    Some(value.to_owned())
}

fn object_fields(value: &Value) -> Vec<String> {
    let Some(map) = value.as_object() else {
        return Vec::new();
    };
    map.keys()
        .filter(|name| {
            !name.is_empty()
                && name.len() <= 64
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_'))
        })
        .map(ToOwned::to_owned)
        .collect()
}

fn label_names(value: Option<&Value>) -> Vec<String> {
    let Some(items) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(Value::as_str)
        .filter(|name| {
            !name.is_empty()
                && name.len() <= 64
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        })
        .map(str::to_owned)
        .collect()
}

#[must_use]
pub fn may_ack(batch: &ParsedBatch, replay_safe: bool) -> bool {
    if batch.message_id < 0 || !replay_safe {
        return false;
    }
    !batch
        .jobs
        .iter()
        .any(|job| matches!(job.kind, InnerKind::Unsupported(_)))
}
