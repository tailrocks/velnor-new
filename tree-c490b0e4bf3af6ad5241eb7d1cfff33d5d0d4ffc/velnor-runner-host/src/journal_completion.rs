//! Durable runner completion identity and cleanup scheduling.

use crate::error::HostError;
use crate::journal::{Journal, intent_row};
use crate::reconcile::IntentRow;

#[path = "journal_completion_assignment.rs"]
mod assignment;
#[path = "journal_completion_claim.rs"]
mod claim;
#[path = "journal_completion_claim_bind.rs"]
mod claim_bind;
#[path = "journal_completion_inbox.rs"]
mod inbox;
#[path = "journal_completion_record.rs"]
mod record;

pub(crate) use inbox::{
    CompletionInboxEntry, MAX_COMPLETION_BODY_BYTES, MAX_COMPLETION_INBOX_SCAN,
};

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

impl Journal {
    /// Read due, completed launches in retry order.
    pub(crate) async fn due_completed_launches(
        &self,
        now: i64,
        limit: u32,
    ) -> Result<Vec<CompletedLaunch>, HostError> {
        if now < 0 {
            return Err(HostError::Journal);
        }
        let connection = self.connection().await?;
        let mut rows = connection
            .query(
                "SELECT i.id, i.kind, i.subject, i.state, i.docker_id, i.github_runner_id, i.cleanup_proven, i.dind_id, i.worker_volume, i.scale_set_id, i.runner_request_id, i.runner_name, i.docker_engine_id, i.launch_phase, i.launch_id, i.assignment_key, i.seed_generation_id, i.acquire_attempted, i.acquire_resolved, i.acquired, i.jit_requested, i.runner_completed, c.scale_set_id, c.runner_request_id, c.runner_id, c.runner_name, c.runner_absent FROM intents AS i JOIN completion_cleanup AS c ON c.intent_id = i.id WHERE i.kind = 'launch' AND i.cleanup_proven = 0 AND i.scale_set_id = c.scale_set_id AND i.runner_name = c.runner_name AND (i.runner_request_id IS NULL OR i.runner_request_id = c.runner_request_id) AND c.retry_after <= ?1 AND c.lease_until <= ?1 ORDER BY c.retry_after, i.id LIMIT ?2",
                (now, i64::from(limit)),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
            out.push(CompletedLaunch {
                intent: intent_row(&row)?,
                identity: CompletionIdentity {
                    scale_set_id: row.get(22).map_err(|_| HostError::Journal)?,
                    runner_request_id: row.get(23).map_err(|_| HostError::Journal)?,
                    runner_id: row.get(24).map_err(|_| HostError::Journal)?,
                    runner_name: row.get(25).map_err(|_| HostError::Journal)?,
                    runner_absent: row.get(26).map_err(|_| HostError::Journal)?,
                },
            });
        }
        Ok(out)
    }
}

pub(super) async fn finish_transaction<T>(
    connection: &turso::Connection,
    result: Result<T, HostError>,
) -> Result<T, HostError> {
    let ended = if result.is_ok() {
        connection.execute("COMMIT", ()).await
    } else {
        connection.execute("ROLLBACK", ()).await
    };
    ended.map_err(|_| HostError::Journal)?;
    result
}

pub(super) fn assigned_request(subject: &str) -> Option<i64> {
    let body = subject.strip_prefix('m')?;
    let (message, request) = body.split_once('r')?;
    if message.is_empty()
        || request.is_empty()
        || !message.bytes().all(|byte| byte.is_ascii_digit())
        || !request.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let message_id = message.parse::<i64>().ok()?;
    let request_id = request.parse::<i64>().ok()?;
    if message_id < 0
        || request_id <= 0
        || message != message_id.to_string()
        || request != request_id.to_string()
    {
        return None;
    }
    Some(request_id)
}

pub(super) fn is_session_name(name: &str) -> bool {
    let Some(suffix) = name.strip_prefix('s') else {
        return false;
    };
    (2..=13).contains(&name.len())
        && !suffix.is_empty()
        && suffix.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

pub(super) fn is_message_name(name: &str) -> bool {
    let Some(suffix) = name.strip_prefix('m') else {
        return false;
    };
    suffix
        .parse::<i64>()
        .is_ok_and(|value| suffix == value.to_string())
}

#[cfg(test)]
#[path = "journal_completion_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "journal_completion_claim_tests.rs"]
mod claim_tests;

#[cfg(test)]
#[path = "journal_completion_inbox_tests.rs"]
mod inbox_tests;

#[cfg(test)]
#[path = "journal_identity_completion_tests.rs"]
mod identity_completion_tests;
