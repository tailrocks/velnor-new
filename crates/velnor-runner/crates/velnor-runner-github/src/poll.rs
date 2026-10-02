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
    let kind = match kind_text {
        "JobAvailable" => InnerKind::Available,
        "JobAssigned" => InnerKind::Assigned,
        "JobStarted" => InnerKind::Started,
        "JobCompleted" => InnerKind::Completed,
        other => InnerKind::Unsupported(other.to_owned()),
    };
    Ok(InnerJob { kind, request_id })
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
