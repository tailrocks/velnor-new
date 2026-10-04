//! Message poll decode. Null and omitted statistics both mean absent.

use serde::Deserialize;
use serde_json::Value;

use crate::error::WireError;

/// One decoded poll.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Poll {
    /// HTTP 202. Not an error and not an acknowledgement.
    Empty,
    /// HTTP 200 envelope.
    Batch(ParsedBatch),
}

/// Inner job kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InnerKind {
    /// `JobAvailable`.
    Available,
    /// `JobAssigned`.
    Assigned,
    /// `JobStarted`.
    Started,
    /// `JobCompleted`.
    Completed,
    /// Any other kind. Visible, and not silently acknowledged.
    Unsupported(String),
}

/// Decoded batch. `statistics == None` covers both JSON null and omission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedBatch {
    /// Service message id. Zero is real. Negative one is synthetic.
    pub message_id: i64,
    /// Assigned-population snapshot, when the service sent one.
    pub statistics: Option<Statistics>,
    /// Parsed inner messages, including unsupported kinds.
    pub jobs: Vec<InnerJob>,
}

/// One inner message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InnerJob {
    /// Kind.
    pub kind: InnerKind,
    /// Validated `runnerRequestId` when present.
    pub request_id: Option<i64>,
    /// Immutable GitHub job metadata from `JobMessageBase`.
    pub context: ImmutableJobContext,
    /// `runnerId` on `JobStarted` and `JobCompleted` only.
    pub runner_id: Option<i64>,
    /// `runnerName` on `JobStarted` and `JobCompleted` only.
    pub runner_name: Option<String>,
    /// `result` on `JobCompleted` only.
    pub result: Option<String>,
    /// Object keys. Names only, so a live trace can show the shape.
    pub fields: Vec<String>,
}

/// Immutable GitHub job metadata from the pinned Scale Set message base.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImmutableJobContext {
    /// GitHub repository name, when sent.
    pub repository_name: Option<String>,
    /// GitHub owner name, when sent.
    pub owner_name: Option<String>,
    /// GitHub job identifier as sent. This is a string in the protocol.
    pub job_id: Option<String>,
    /// Workflow file reference, when sent.
    pub job_workflow_ref: Option<String>,
    /// Display-only job name. It is not a source identity.
    pub job_display_name: Option<String>,
    /// Workflow run identifier, when sent.
    pub workflow_run_id: Option<i64>,
    /// Triggering event name, when sent.
    pub event_name: Option<String>,
    /// `requestLabels` exactly as sent. Empty when omitted or null.
    pub request_labels: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobMessageBase {
    message_type: String,
    runner_request_id: Option<i64>,
    repository_name: Option<String>,
    owner_name: Option<String>,
    job_id: Option<String>,
    job_workflow_ref: Option<String>,
    job_display_name: Option<String>,
    workflow_run_id: Option<i64>,
    event_name: Option<String>,
    request_labels: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobStartedFields {
    runner_id: Option<i64>,
    runner_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobCompletedFields {
    runner_id: Option<i64>,
    runner_name: Option<String>,
    result: Option<String>,
}

/// `statistics` object from the pinned Go struct.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Statistics {
    /// Jobs waiting.
    pub total_available_jobs: i64,
    /// Jobs acquired.
    pub total_acquired_jobs: i64,
    /// Population authority. Not the length of this batch.
    pub total_assigned_jobs: i64,
    /// Jobs running.
    pub total_running_jobs: i64,
    /// Registered runners.
    pub total_registered_runners: i64,
    /// Busy runners.
    pub total_busy_runners: i64,
    /// Idle runners.
    pub total_idle_runners: i64,
}

impl Statistics {
    /// Assigned population. Do not add the batch length to this.
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
    body: String,
    #[serde(default)]
    statistics: Option<Statistics>,
}

/// Decode a poll. HTTP 202 ignores the body.
///
/// # Errors
///
/// Returns [`WireError::Malformed`] or [`WireError::UnsupportedEnvelope`].
pub fn parse_poll(status: u16, body: &str) -> Result<Poll, WireError> {
    if status == 202 {
        return Ok(Poll::Empty);
    }
    if status != 200 {
        return Err(WireError::UnexpectedStatus);
    }
    parse_envelope(body).map(Poll::Batch)
}

fn parse_envelope(body: &str) -> Result<ParsedBatch, WireError> {
    let envelope: Envelope = serde_json::from_str(body).map_err(|_| WireError::Malformed)?;
    if envelope.message_type != "RunnerScaleSetJobMessages" {
        return Err(WireError::UnsupportedEnvelope);
    }
    let jobs = parse_body(&envelope.body)?;
    Ok(ParsedBatch {
        message_id: envelope.message_id,
        statistics: envelope.statistics,
        jobs,
    })
}

fn parse_body(body: &str) -> Result<Vec<InnerJob>, WireError> {
    if body.is_empty() {
        return Ok(Vec::new());
    }
    let values: Vec<Value> = serde_json::from_str(body).map_err(|_| WireError::Malformed)?;
    values.iter().map(parse_inner).collect()
}

fn parse_inner(value: &Value) -> Result<InnerJob, WireError> {
    let base: JobMessageBase = decode_message(value)?;
    let request_id = base.runner_request_id;
    let context = ImmutableJobContext {
        repository_name: base.repository_name,
        owner_name: base.owner_name,
        job_id: base.job_id,
        job_workflow_ref: base.job_workflow_ref,
        job_display_name: base.job_display_name,
        workflow_run_id: base.workflow_run_id,
        event_name: base.event_name,
        request_labels: base.request_labels.unwrap_or_default(),
    };
    let fields = object_fields(value);
    let kind = match base.message_type.as_str() {
        "JobAvailable" => InnerKind::Available,
        "JobAssigned" => InnerKind::Assigned,
        "JobStarted" => InnerKind::Started,
        "JobCompleted" => InnerKind::Completed,
        other => InnerKind::Unsupported(other.to_owned()),
    };
    let runner_fields = message_runner_fields(value, &kind)?;
    Ok(InnerJob {
        kind,
        request_id,
        context,
        runner_id: runner_fields.runner_id,
        runner_name: runner_fields.runner_name,
        result: runner_fields.result,
        fields,
    })
}

fn message_runner_fields(
    value: &Value,
    kind: &InnerKind,
) -> Result<MessageRunnerFields, WireError> {
    match kind {
        InnerKind::Started => {
            let started: JobStartedFields = decode_message(value)?;
            Ok(MessageRunnerFields {
                runner_id: started.runner_id,
                runner_name: started.runner_name,
                result: None,
            })
        }
        InnerKind::Completed => {
            let completed: JobCompletedFields = decode_message(value)?;
            Ok(MessageRunnerFields {
                runner_id: completed.runner_id,
                runner_name: completed.runner_name,
                result: completed.result,
            })
        }
        InnerKind::Available | InnerKind::Assigned | InnerKind::Unsupported(_) => {
            Ok(MessageRunnerFields::default())
        }
    }
}

fn decode_message<T: for<'de> Deserialize<'de>>(value: &Value) -> Result<T, WireError> {
    serde_json::from_value(value.clone()).map_err(|_| WireError::Malformed)
}

#[derive(Default)]
struct MessageRunnerFields {
    runner_id: Option<i64>,
    runner_name: Option<String>,
    result: Option<String>,
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

/// Acknowledge only a real id, and never a batch that still has an unknown kind.
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
