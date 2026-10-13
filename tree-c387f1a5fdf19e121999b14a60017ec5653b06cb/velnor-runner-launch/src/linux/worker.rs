//! Journal callbacks at every durable Linux worker side-effect boundary.

use velnor_runner_host::HostError;
use velnor_runner_host::RunnerImageProfile;
use velnor_runner_host::stage::{PairEngine, PairSink, RunnerStartRequirement};
use velnor_runner_host::worker::start_pair_with_profile_and_sink;
use velnor_runner_journal::journal::Journal;

pub(super) struct JournalPairSink<'a> {
    pub(super) journal: &'a Journal,
    pub(super) launch_id: i64,
}

impl PairSink for JournalPairSink<'_> {
    async fn volume(&self, volume: &str) -> Result<(), HostError> {
        self.journal
            .bind_worker_volume(self.launch_id, volume)
            .await
    }

    async fn dind(&self, id: &str) -> Result<(), HostError> {
        self.journal
            .bind_worker(self.launch_id, None, Some(id))
            .await
    }

    async fn runner(&self, id: &str) -> Result<(), HostError> {
        self.journal
            .bind_worker(self.launch_id, Some(id), None)
            .await
    }

    async fn outer_network_intent(&self, name: &str) -> Result<(), HostError> {
        self.journal
            .record_outer_network_intent(self.launch_id, name)
            .await
    }

    async fn outer_network(&self, id: &str) -> Result<(), HostError> {
        self.journal.bind_outer_network_id(self.launch_id, id).await
    }

    async fn before_runner_start(
        &self,
        id: &str,
        requirement: RunnerStartRequirement,
    ) -> Result<(), HostError> {
        if requirement != RunnerStartRequirement::DurableRequired {
            return Err(HostError::Journal);
        }
        if self.journal.draining().await? {
            return Err(HostError::Journal);
        }
        self.journal
            .record_runner_start_intent(self.launch_id, id)
            .await
    }
}

/// Start one profile-pinned pair through Journal's exact-ID callbacks.
pub(super) async fn start_worker_pair<E: PairEngine>(
    docker: &E,
    journal: &Journal,
    launch_id: i64,
    volume: &str,
    profile: &RunnerImageProfile,
    jit: &[u8],
) -> Result<(), HostError> {
    let sink = JournalPairSink { journal, launch_id };
    match start_pair_with_profile_and_sink(docker, volume, jit, profile, &sink).await {
        Ok(_) => Ok(()),
        Err(failure) => {
            if persist_partial(journal, launch_id, failure.partial()).await {
                Err(failure.error())
            } else {
                Err(HostError::Journal)
            }
        }
    }
}

async fn persist_partial(
    journal: &Journal,
    launch_id: i64,
    partial: &velnor_runner_host::stage::PartialPair,
) -> bool {
    let mut persisted = true;
    if let Some(network_id) = partial.outer_network_id.as_deref() {
        persisted &= journal
            .bind_outer_network_id(launch_id, network_id)
            .await
            .is_ok();
    }
    if partial.dind_id.is_some() || partial.runner_id.is_some() {
        persisted &= journal
            .bind_worker(
                launch_id,
                partial.runner_id.as_deref(),
                partial.dind_id.as_deref(),
            )
            .await
            .is_ok();
    }
    persisted
}
