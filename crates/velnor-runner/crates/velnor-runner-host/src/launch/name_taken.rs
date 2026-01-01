//! JIT HTTP 409 for runner `m{message_id}`.
//!
//! The name is already registered. Another mint cannot succeed. Acknowledge the
//! scale or assigned replay so the listener can read the next message. Fail only
//! an unstarted row. A row that already has a container keeps that container.

use velnor_runner_github::Poll;

use crate::error::HostError;
use crate::journal::IntentState;
use crate::journal::{Journal, Outcome};
use crate::scale_set::EnsureError;

use super::steps::Idle;

/// True when a JIT name collision may be acknowledged.
///
/// `Idle::Scale` already has a runner name. The assigned count does not
/// block the acknowledgement. A live container stays occupied.
#[must_use]
pub(crate) fn should_ack(idle: Idle) -> bool {
    matches!(idle, Idle::Scale)
}

/// Fail an unstarted `m{message_id}` row. Return true only when every match has no container id.
///
/// A runner id or a `DinD` id means a container exists. Do not fail that row.
///
/// # Errors
///
/// Returns [`EnsureError`] when the journal read or the state write fails.
pub(crate) async fn fail_unstarted(journal: &Journal, polled: &Poll) -> Result<bool, EnsureError> {
    let Poll::Batch(batch) = polled else {
        return Ok(false);
    };
    let subject = format!("m{}", batch.message_id);
    let rows = journal.rows().await.map_err(map_journal)?;
    let mut matched = false;
    for row in &rows {
        if row.kind != "launch" || row.subject != subject {
            continue;
        }
        if row.docker_id.is_some() || row.dind_id.is_some() {
            return Ok(false);
        }
        matched = true;
    }
    if !matched {
        return Ok(false);
    }
    for row in rows {
        if row.kind != "launch" || row.subject != subject || row.state != IntentState::Uncertain {
            continue;
        }
        journal
            .finish(row.id, Outcome::DefiniteFailure)
            .await
            .map_err(map_journal)?;
    }
    Ok(true)
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

#[cfg(test)]
mod tests {
    use velnor_runner_github::{ParsedBatch, Poll};

    use super::*;
    use crate::journal::IntentState;
    use crate::journal::Outcome;
    use crate::launch_harness::{absent, open};

    #[tokio::test]
    async fn dind_row_is_not_failed() -> Result<(), String> {
        let (scratch, journal) = open("dind-kept").await?;
        let id = journal
            .begin("launch", "m100000776")
            .await
            .map_err(|err| err.to_string())?;
        journal
            .bind_worker(id, None, Some("dind-1"))
            .await
            .map_err(|err| err.to_string())?;
        journal
            .finish(id, Outcome::Uncertain)
            .await
            .map_err(|err| err.to_string())?;
        let poll = Poll::Batch(ParsedBatch {
            message_id: 100_000_776,
            raw_body: String::new(),
            statistics: None,
            jobs: Vec::new(),
        });
        let failed = fail_unstarted(&journal, &poll)
            .await
            .map_err(|err| err.to_string())?;
        if failed {
            return Err("dind row was failed".to_owned());
        }
        let state = journal.read(id).await.map_err(|err| err.to_string())?;
        if state != IntentState::Uncertain {
            return Err(format!("state {state:?}"));
        }
        absent(&scratch.file())
    }
}
