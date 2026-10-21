//! Durable runner completion identity and cleanup scheduling.

use crate::reconcile::IntentRow;

pub(crate) const MAX_COMPLETION_BODY_BYTES: usize = velnor_runner_github::MAX_POLL_BODY_BYTES;
pub(crate) const MAX_COMPLETION_INBOX_SCAN: u32 = 4;

/// One leased completion cleanup attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CleanupClaim {
    /// Monotonic fencing token.
    pub(crate) generation: i64,
    /// Bounded retry count used by the caller to select a backoff delay.
    pub(crate) attempt: u32,
}

/// Durable Scale Set and runner identity recorded for one completion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CompletionIdentity {
    /// Owning runner scale set.
    pub(crate) scale_set_id: i64,
    /// Request id from the completion event.
    pub(crate) runner_request_id: i64,
    /// Official Actions runner id from the completion event.
    pub(crate) runner_id: i64,
    /// Exact generated runner name.
    pub(crate) runner_name: String,
    /// Whether an exact-name official runner lookup confirmed absence after deletion.
    pub(crate) runner_absent: bool,
}

/// One completion intent and its persisted identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CompletedLaunch {
    /// Original host intent.
    pub(crate) intent: IntentRow,
    /// Identity that the cleanup caller must validate before deleting a runner.
    pub(crate) identity: CompletionIdentity,
}
