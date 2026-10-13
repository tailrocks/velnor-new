use serde_json::{Map, Value};

use crate::{InnerJob, Poll, Statistics, WireError, parse_poll};

use super::types::WorkflowTrustField;

const MAX_TRUST_TEXT: usize = 4096;

/// Poll result with each parsed event and its pinned `jobWorkflowRef` kept
/// together in an immutable event record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PollWithTrust {
    /// HTTP 202; no messages were delivered.
    Empty,
    /// HTTP 200 with parsed messages and their trust-relevant fields.
    Batch(ParsedTrustBatch),
}

/// One protocol event and its trust metadata, kept together and immutable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedTrustEvent {
    job: InnerJob,
    job_workflow_ref: WorkflowTrustField<String>,
}

impl ParsedTrustEvent {
    /// Decoded event whose workflow metadata belongs to this same event.
    #[must_use]
    pub const fn job(&self) -> &InnerJob {
        &self.job
    }

    /// Exact official `jobWorkflowRef`, without parsing or normalization.
    #[must_use]
    pub const fn job_workflow_ref(&self) -> &WorkflowTrustField<String> {
        &self.job_workflow_ref
    }
}

/// Immutable protocol batch whose events keep each job paired with its trust metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedTrustBatch {
    message_id: i64,
    statistics: Option<Statistics>,
    events: Vec<ParsedTrustEvent>,
}

impl ParsedTrustBatch {
    /// Service message ID. A negative value is reserved for synthetic startup messages.
    #[must_use]
    pub const fn message_id(&self) -> i64 {
        self.message_id
    }

    /// Assigned-population statistics, if supplied by the service.
    #[must_use]
    pub const fn statistics(&self) -> Option<&Statistics> {
        self.statistics.as_ref()
    }

    /// Events and their paired trust metadata, in source order.
    #[must_use]
    pub fn events(&self) -> &[ParsedTrustEvent] {
        &self.events
    }

    /// One event and its paired trust metadata.
    #[must_use]
    pub fn event(&self, index: usize) -> Option<&ParsedTrustEvent> {
        self.events.get(index)
    }
}

/// Decode a poll while retaining `jobWorkflowRef` from the pinned message
/// contract. Each job and its metadata are stored as one immutable event; the
/// result does not make any admission decision by itself.
///
/// # Errors
///
/// Returns [`WireError::Malformed`] or another parser error for invalid poll
/// envelopes. A missing or malformed `jobWorkflowRef` is retained as typed
/// evidence rather than causing a message to be dropped.
pub fn parse_poll_with_trust(status: u16, body: &str) -> Result<PollWithTrust, WireError> {
    match parse_poll(status, body)? {
        Poll::Empty => Ok(PollWithTrust::Empty),
        Poll::Batch(batch) => {
            let workflow_refs = parse_message_trust_fields(body)?;
            if workflow_refs.len() != batch.jobs.len() {
                return Err(WireError::Malformed);
            }
            let events = batch
                .jobs
                .into_iter()
                .zip(workflow_refs)
                .map(|(job, job_workflow_ref)| ParsedTrustEvent {
                    job,
                    job_workflow_ref,
                })
                .collect();
            Ok(PollWithTrust::Batch(ParsedTrustBatch {
                message_id: batch.message_id,
                statistics: batch.statistics,
                events,
            }))
        }
    }
}

fn parse_message_trust_fields(body: &str) -> Result<Vec<WorkflowTrustField<String>>, WireError> {
    let envelope: Value = serde_json::from_str(body).map_err(|_| WireError::Malformed)?;
    let inner = envelope
        .get("body")
        .and_then(Value::as_str)
        .ok_or(WireError::Malformed)?;
    if inner.is_empty() {
        return Ok(Vec::new());
    }
    let values: Vec<Value> = serde_json::from_str(inner).map_err(|_| WireError::Malformed)?;
    values
        .iter()
        .map(|value| {
            let object = value.as_object().ok_or(WireError::Malformed)?;
            Ok(field_string(object, "jobWorkflowRef"))
        })
        .collect()
}

fn field_string(object: &Map<String, Value>, key: &str) -> WorkflowTrustField<String> {
    match object.get(key) {
        None | Some(Value::Null) => WorkflowTrustField::Missing,
        Some(Value::String(value)) if valid_trust_text(value) => {
            WorkflowTrustField::Present(value.clone())
        }
        Some(_) => WorkflowTrustField::Invalid,
    }
}

fn valid_trust_text(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_TRUST_TEXT && !value.chars().any(char::is_control)
}
