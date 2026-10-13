//! Reconcile completed jobs before admitting work from the next poll.

use velnor_runner_github::{InnerJob, InnerKind};

use crate::journal::Journal;
use crate::scale_set::EnsureError;

mod reconcile;
mod schedule;
mod support;
#[cfg(test)]
mod tests;

pub(in crate::launch) use schedule::schedule_completed;
#[cfg(test)]
pub(in crate::launch) use schedule::schedule_completed_isolated;

use support::{completion_error, map_journal};

pub(super) async fn record_completion_events(
    journal: &Journal,
    set_id: i64,
    polled: &velnor_runner_github::Poll,
) -> Result<(), EnsureError> {
    if let velnor_runner_github::Poll::Batch(batch) = polled {
        for job in &batch.jobs {
            if matches!(job.kind, InnerKind::Completed) {
                record_completion(journal, set_id, job).await?;
            }
        }
    }
    Ok(())
}

async fn record_completion(
    journal: &Journal,
    set_id: i64,
    job: &InnerJob,
) -> Result<(), EnsureError> {
    let Some(request_id) = job.request_id.filter(|id| *id >= 0) else {
        return Err(completion_error());
    };
    let Some(runner_id) = job.runner_id.filter(|id| *id > 0) else {
        return Err(completion_error());
    };
    let Some(runner_name) = job.runner_name.as_deref() else {
        return Err(completion_error());
    };
    journal
        .record_runner_completed(set_id, request_id, runner_id, runner_name)
        .await
        .map_err(map_journal)?;
    Ok(())
}
