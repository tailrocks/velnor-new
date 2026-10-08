//! Preserve exact current statistics before processing or acknowledging a batch.

use velnor_runner_github::policy::ParsedTrustBatch;
use velnor_runner_github::{PopulationObservationSource, SessionPopulationObservation};
use velnor_runner_journal::journal::ScaleSetPopulationSnapshot;

use super::super::{cutoff, protocol_read};
use super::BatchWork;

pub(super) async fn persist_poll_observation(
    work: &mut BatchWork<'_>,
    batch: &ParsedTrustBatch,
) -> bool {
    let observation = cutoff::bounded_persisting(
        work.journal,
        &mut work.shutdown,
        work.context.drain_timeout(),
        None,
        protocol_read(work.active.protocol.clone(), |protocol| {
            protocol.session.population_observation().cloned()
        }),
    )
    .await;
    let Some(Ok(observation)) = observation else {
        return false;
    };
    let Some(observation) = observation else {
        return batch.statistics().is_none();
    };
    if !matches_batch_observation(batch, work.active.binding.scale_set_id, &observation) {
        return false;
    }
    let Ok(snapshot) =
        ScaleSetPopulationSnapshot::from_observation(work.active.intent_id, &observation)
    else {
        return false;
    };
    let saved = cutoff::bounded_persisting(
        work.journal,
        &mut work.shutdown,
        work.context.drain_timeout(),
        None,
        work.journal
            .record_scale_set_population_snapshot(&work.active.identity, &snapshot),
    )
    .await;
    matches!(saved, Some(Ok(_)))
}

fn matches_batch_observation(
    batch: &ParsedTrustBatch,
    scale_set_id: i64,
    observation: &SessionPopulationObservation,
) -> bool {
    observation.source() == PopulationObservationSource::PollBatch
        && Some(observation.session_id()) == batch.source_session_id()
        && observation.scale_set_id() == scale_set_id
        && batch.source_scale_set_id() == Some(scale_set_id)
        && observation.message_id() == Some(batch.message_id())
        && batch.statistics() == Some(observation.statistics())
}
