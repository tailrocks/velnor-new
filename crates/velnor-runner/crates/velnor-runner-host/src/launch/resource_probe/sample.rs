//! Non-serializable results scoped to one completed probe transaction.

use std::time::Instant;

use crate::worker::ResourceBudget;

#[cfg(not(test))]
use crate::launch::pressure::Sample;

use super::record::ProbeRecord;

/// A validated selected-guest sample that cannot survive a process restart.
pub(super) struct Observation {
    record: ProbeRecord,
    #[cfg(not(test))]
    cpus: u32,
    engine_id: String,
    docker_root_digest: String,
    observed_at: Instant,
}

impl Observation {
    pub(super) fn new(
        record: ProbeRecord,
        cpus: u32,
        memory_total_bytes: u64,
        engine_id: String,
        docker_root_digest: String,
        observed_at: Instant,
    ) -> Option<Self> {
        (cpus > 0
            && memory_total_bytes > 0
            && record.memory_available_bytes <= memory_total_bytes
            && !engine_id.is_empty())
        .then_some(Self {
            record,
            #[cfg(not(test))]
            cpus,
            engine_id,
            docker_root_digest,
            observed_at,
        })
    }

    #[cfg(not(test))]
    pub(super) fn pressure(self) -> Sample {
        Sample {
            load_millis: self.record.load_milli,
            ncpu: self.cpus,
            mem_available: self.record.memory_available_bytes,
            disk_free: self.record.docker_root_free_bytes,
        }
    }

    pub(super) fn into_start_permit(self, budget: ResourceBudget) -> Option<StartPermit> {
        let pair = budget.pair();
        let recent = self.observed_at.elapsed().as_secs() <= 30;
        (recent
            && self.record.docker_root_free_bytes >= 10 * 1024 * 1024 * 1024
            && self.record.memory_available_bytes >= pair.memory_bytes)
            .then_some(StartPermit(self))
    }
}

/// Single-use token for the start boundary; it is neither cloneable nor serializable.
pub(crate) struct StartPermit(Observation);

impl StartPermit {
    pub(super) fn consume(self, current_engine_id: &str, current_root_digest: &str) -> bool {
        self.0.engine_id == current_engine_id
            && self.0.docker_root_digest == current_root_digest
            && self.0.observed_at.elapsed().as_secs() <= 30
    }

    #[cfg(test)]
    pub(super) fn test_fixture() -> Self {
        let docker_root_digest = super::projection::DockerRoot::parse("/var/lib/docker")
            .map(|root| root.digest().to_owned())
            .unwrap_or_default();
        StartPermit(Observation {
            record: ProbeRecord {
                schema_version: 1,
                docker_root_free_bytes: 20 * 1024 * 1024 * 1024,
                docker_root_total_bytes: 40 * 1024 * 1024 * 1024,
                memory_available_bytes: 8 * 1024 * 1024 * 1024,
                load_milli: 125,
                memory_psi_some_avg10_bps: None,
            },
            engine_id: "test-engine".to_owned(),
            docker_root_digest,
            observed_at: Instant::now(),
        })
    }
}
