//! JIT HTTP 409 for runner `m{message_id}` when the session census has no assigned job.
//!
//! The name is already registered. A new mint loops. Acknowledge the replay and
//! fail the unstarted row so that row does not keep a slot.

use velnor_runner_github::Poll;

use crate::error::HostError;
use crate::journal::{Journal, Outcome};
use crate::journal::IntentState;
use crate::scale_set::EnsureError;

use super::steps::Idle;

/// True when a scale replay may be acknowledged after JIT returns HTTP 409.
#[must_use]
pub(crate) fn should_ack(idle: Idle, live_assigned: Option<i64>) -> bool {
    live_assigned == Some(0) && idle == Idle::Scale
}

/// Mark an unstarted `m{message_id}` row failed. A row with a container id stays.
///
/// # Errors
///
/// Returns [`EnsureError`] when the journal read or the state write fails.
pub(crate) async fn fail_unstarted(journal: &Journal, polled: &Poll) -> Result<(), EnsureError> {
    let Poll::Batch(batch) = polled else {
        return Ok(());
    };
    let subject = format!("m{}", batch.message_id);
    let rows = journal.rows().await.map_err(map_journal)?;
    for row in rows {
        if row.kind != "launch" || row.subject != subject || row.docker_id.is_some() {
            continue;
        }
        if row.state != IntentState::Uncertain {
            continue;
        }
        journal
            .finish(row.id, Outcome::DefiniteFailure)
            .await
            .map_err(map_journal)?;
    }
    Ok(())
}

fn map_journal(error: HostError) -> EnsureError {
    match error {
        HostError::Endpoint => EnsureError::Endpoint,
        _ => EnsureError::Unexpected {
            status: 0,
            step: "journal",
        },
    }
}
