//! Message poll decode. Null and omitted statistics both mean absent.

use serde::Deserialize;
use serde_json::Value;

use crate::error::WireError;

/// Local byte bound for one Scale Set poll body.
pub const MAX_POLL_BODY_BYTES: usize = 256 * 1024;
/// Maximum message count accepted from one Scale Set poll.
pub const MAX_POLL_MESSAGES: usize = 50;
const MAX_POLL_ENVELOPE_BYTES: usize = MAX_POLL_BODY_BYTES * 8;

/// One decoded poll.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Poll {
    /// HTTP 202. Not an error and not an acknowledgement.
    Empty,
    /// HTTP 200 envelope.
    Batch(ParsedBatch),
    /// A valid outer envelope whose bounded inner body could not be decoded.
    /// The host must persist this exact body under the outer ID before ACK.
    Quarantined(QuarantinedBatch),
}

/// Malformed inner body retained under its stable outer Scale Set message ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuarantinedBatch {
    /// The outer `runnerScaleSetMessageResponse.messageId`.
    pub message_id: i64,
    /// Exact decoded outer `body` string, bounded by [`MAX_POLL_BODY_BYTES`].
    pub raw_body: String,
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
    /// Exact inner message body. Keep it when ownership needs quarantine.
    pub raw_body: String,
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
    /// `runnerId` on `JobStarted` and `JobCompleted` only.
    pub runner_id: Option<i64>,
    /// `runnerName` on `JobStarted` and `JobCompleted` only.
    pub runner_name: Option<String>,
    /// `result` on `JobCompleted` only.
    pub result: Option<String>,
    /// Numeric `jobId` only. Other shapes are dropped.
    pub job_id: Option<String>,
    /// `requestLabels` names. Empty when the field is absent.
    pub labels: Vec<String>,
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

/// Decode a poll. HTTP 202 ignores the body. A bounded malformed inner body
/// is returned as [`Poll::Quarantined`] when its outer message ID is stable.
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
    let raw_body = envelope.body;
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

/// Parse a persisted exact inner message body for bounded inbox recovery.
///
/// # Errors
///
/// Returns [`WireError::Malformed`] when JSON is invalid or exceeds a byte or message limit.
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
    Ok(InnerJob {
        kind,
        request_id,
        runner_id: runner_fields.runner_id,
        runner_name: runner_fields.runner_name,
        result: runner_fields.result,
        job_id,
        labels,
        fields,
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
        Value::String(text) => numeric_job_id(Some(text)),
        Value::Number(number) => positive_job_id(number.as_i64()),
        _ => None,
    }
}

fn positive_job_id(value: Option<i64>) -> Option<String> {
    let number = value.filter(|item| *item > 0)?;
    Some(number.to_string())
}

fn numeric_job_id(value: Option<&str>) -> Option<String> {
    let text = value?;
    if text.is_empty() || text.len() > 24 || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    if text.bytes().all(|byte| byte == b'0') {
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
