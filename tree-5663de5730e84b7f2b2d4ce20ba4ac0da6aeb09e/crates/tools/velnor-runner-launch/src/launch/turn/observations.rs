//! Durable observations from each actual queue poll.

use velnor_runner_github::Poll;
use velnor_runner_host::scale_set::EnsureError;
use velnor_runner_journal::journal::Journal;

/// Persist actual runner lifecycle identities before admission can hold, stop, or acknowledge.
pub(super) async fn persist_observed_lifecycle(
    journal: &Journal,
    polled: &Poll,
) -> Result<(), EnsureError> {
    let Poll::Batch(batch) = polled else {
        return Ok(());
    };
    for event in &batch.jobs {
        journal
            .observe_runner_event(event)
            .await
            .map_err(map_journal)?;
    }
    Ok(())
}

fn map_journal(error: velnor_runner_host::HostError) -> EnsureError {
    match error {
        velnor_runner_host::HostError::Endpoint => EnsureError::Endpoint,
        _ => EnsureError::Unexpected {
            status: 0,
            step: "journal",
        },
    }
}
