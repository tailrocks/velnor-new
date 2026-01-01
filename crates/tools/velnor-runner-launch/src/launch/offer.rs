//! Acquire, JIT, start, and acknowledge a single queue offer.

use std::future::Future;

use velnor_runner_github::Poll;
use velnor_runner_host::{HostError, scale_set::EnsureError, worker::Started};
use velnor_runner_journal::journal::Journal;

use super::{Drive, Lane, bind, steps};

/// Acquire, JIT, and start for one poll. No session create.
///
/// # Errors
///
/// Returns [`EnsureError`] when more than one job is offered, or a later step fails.
/// An uncertain acquire is not acknowledged.
#[cfg(test)]
pub(crate) async fn drive_offer<T, S, F>(
    lane: &mut T,
    ctx: &Drive,
    polled: &Poll,
    journal: &Journal,
    start: S,
) -> Result<Option<Started>, EnsureError>
where
    T: velnor_runner_github::Transport + Lane,
    S: FnOnce(&str, &[u8], bind::Bind) -> F,
    F: Future<Output = Result<Started, HostError>>,
{
    drive_offer_tracked(lane, ctx, polled, journal, start)
        .await
        .map(|outcome| outcome.started)
}

/// Worker start plus the queue message deleted by this exact successful offer.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct DriveOutcome {
    /// Worker newly started by the offer, if any.
    pub(crate) started: Option<Started>,
    /// Message whose DELETE returned success after the worker path completed.
    pub(crate) acknowledged_message_id: Option<i64>,
}

/// Same start path as the test-only convenience wrapper, retaining positive ACK evidence.
pub(crate) async fn drive_offer_tracked<T, S, F>(
    lane: &mut T,
    ctx: &Drive,
    polled: &Poll,
    journal: &Journal,
    start: S,
) -> Result<DriveOutcome, EnsureError>
where
    T: velnor_runner_github::Transport + Lane,
    S: FnOnce(&str, &[u8], bind::Bind) -> F,
    F: Future<Output = Result<Started, HostError>>,
{
    if matches!(steps::idle(polled), steps::Idle::Scale) {
        let Poll::Batch(batch) = polled else {
            return Ok(DriveOutcome::default());
        };
        let started = steps::scale_id(lane, ctx, batch, journal, start).await?;
        return Ok(DriveOutcome {
            started,
            acknowledged_message_id: Some(batch.message_id),
        });
    }
    let Some((batch, request_id)) = steps::assignment(polled)? else {
        return Ok(DriveOutcome::default());
    };
    let started = steps::launch_id(lane, ctx, batch, journal, request_id, start).await?;
    Ok(DriveOutcome {
        started,
        acknowledged_message_id: Some(batch.message_id),
    })
}
