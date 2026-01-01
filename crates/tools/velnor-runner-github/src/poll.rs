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
    /// `runnerRequestId` when present.
    pub request_id: Option<i64>,
    /// Opaque `jobId` supplied as a string by the Scale Set message.
    pub job_id: Option<String>,
    /// `workflowRunId` supplied by the Scale Set message.
    pub workflow_run_id: Option<i64>,
    /// Base repository owner from `ownerName`.
    pub owner_name: Option<String>,
    /// Base repository name from `repositoryName`.
    pub repository_name: Option<String>,
    /// Trigger event from `eventName`.
    pub event_name: Option<String>,
    /// `requestLabels` names. Empty when the field is absent.
    pub labels: Vec<String>,
    /// `runnerId` on `JobStarted` and `JobCompleted` when present.
    pub runner_id: Option<i64>,
    /// `runnerName` on `JobStarted` and `JobCompleted` when present.
    pub runner_name: Option<String>,
    /// `result` on `JobCompleted` when present.
    pub result: Option<String>,
    /// Object keys. Names only, so a live trace can show the shape.
    pub fields: Vec<String>,
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
    let kind_text = value
        .get("messageType")
        .and_then(Value::as_str)
        .ok_or(WireError::Malformed)?;
    let request_id = value.get("runnerRequestId").and_then(Value::as_i64);
    let job_id = bounded_text(value.get("jobId").and_then(Value::as_str), 256);
    let workflow_run_id = value
        .get("workflowRunId")
        .and_then(Value::as_i64)
        .filter(|id| *id > 0);
    let owner_name = bounded_text(value.get("ownerName").and_then(Value::as_str), 100);
    let repository_name = bounded_text(value.get("repositoryName").and_then(Value::as_str), 100);
    let event_name = bounded_text(value.get("eventName").and_then(Value::as_str), 100);
    let labels = label_names(value.get("requestLabels"));
    let runner_id = value.get("runnerId").and_then(Value::as_i64);
    let runner_name = value
        .get("runnerName")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let result = value
        .get("result")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let fields = object_fields(value);
    let kind = match kind_text {
        "JobAvailable" => InnerKind::Available,
        "JobAssigned" => InnerKind::Assigned,
        "JobStarted" => InnerKind::Started,
        "JobCompleted" => InnerKind::Completed,
        other => InnerKind::Unsupported(other.to_owned()),
    };
    Ok(InnerJob {
        kind,
        request_id,
        job_id,
        workflow_run_id,
        owner_name,
        repository_name,
        event_name,
        labels,
        runner_id,
        runner_name,
        result,
        fields,
    })
}

fn bounded_text(value: Option<&str>, maximum: usize) -> Option<String> {
    let text = value?;
    if text.is_empty() || text.len() > maximum || text.chars().any(char::is_control) {
        return None;
    }
    Some(text.to_owned())
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
