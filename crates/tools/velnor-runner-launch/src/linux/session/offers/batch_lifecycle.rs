use velnor_runner_github::policy::ParsedTrustBatch;

use super::{BatchKinds, BatchWork, cleanup, lifecycle, recovery, snapshot};
use crate::linux::session::{cutoff, observe_cutoff};

pub(super) async fn persist_and_clean_lifecycle(
    work: &mut BatchWork<'_>,
    batch: &ParsedTrustBatch,
) -> Option<BatchKinds> {
    let current = cutoff::bounded_persisting(
        work.journal,
        &mut work.shutdown,
        work.context.drain_timeout(),
        None,
        super::batch_is_current(work.active, batch),
    )
    .await;
    observe_cutoff(
        work.context,
        work.journal,
        work.shutdown.receiver,
        work.shutdown.cutoff,
    )
    .await;
    if !matches!(current, Some(true)) {
        return None;
    }
    if !snapshot::persist_poll_observation(work, batch).await {
        return None;
    }
    observe_cutoff(
        work.context,
        work.journal,
        work.shutdown.receiver,
        work.shutdown.cutoff,
    )
    .await;
    if work.shutdown.cutoff.is_some() {
        return None;
    }
    let kinds = BatchKinds::from(batch);
    let persisted = cutoff::bounded_persisting(
        work.journal,
        &mut work.shutdown,
        work.context.drain_timeout(),
        None,
        lifecycle::persist_events(work.journal, batch),
    )
    .await;
    observe_cutoff(
        work.context,
        work.journal,
        work.shutdown.receiver,
        work.shutdown.cutoff,
    )
    .await;
    let Some(Some(mut completed)) = persisted else {
        return None;
    };
    let recovered = recovery::recover_pending(work).await?;
    completed.extend(recovered);
    if !completed.is_empty()
        && !Box::pin(cleanup::cleanup_completed(
            work.context,
            work.journal,
            work.diagnostics,
            &work.active.docker_binding,
            &mut work.shutdown,
            &completed,
        ))
        .await
    {
        return None;
    }
    if work.shutdown.cutoff.is_some() {
        return None;
    }
    Some(kinds)
}
