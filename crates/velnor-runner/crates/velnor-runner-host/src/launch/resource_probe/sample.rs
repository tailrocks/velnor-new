use std::time::{Duration, Instant};

use crate::worker::ResourceBudget;

use crate::launch::pressure::Sample;

use super::record::ProbeRecord;

const MAX_OBSERVATION_AGE: Duration = Duration::from_secs(30);

pub(super) struct Observation {
    record: ProbeRecord,
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
            cpus,
            engine_id,
            docker_root_digest,
            observed_at,
        })
    }

    pub(super) fn is_recent_at(&self, now: Instant) -> bool {
        now.checked_duration_since(self.observed_at)
            .is_some_and(|age| age <= MAX_OBSERVATION_AGE)
    }

    pub(super) fn pressure(self) -> Option<Sample> {
        self.is_recent_at(Instant::now()).then_some(Sample {
            load_millis: self.record.load_milli,
            ncpu: self.cpus,
            mem_available: self.record.memory_available_bytes,
            disk_free: self.record.docker_root_free_bytes,
        })
    }

    pub(super) fn into_start_permit(self, budget: ResourceBudget) -> Option<StartPermit> {
        let pair = budget.pair();
        let recent = self.is_recent_at(Instant::now());
        (recent
            && self.record.docker_root_free_bytes >= 10 * 1024 * 1024 * 1024
            && self.record.memory_available_bytes >= pair.memory_bytes)
            .then_some(StartPermit(self))
    }
}

pub(crate) struct StartPermit(Observation);

impl StartPermit {
    pub(super) fn consume(self, current_engine_id: &str, current_root_digest: &str) -> bool {
        self.is_valid_at(current_engine_id, current_root_digest, Instant::now())
    }

    fn is_valid_at(&self, engine_id: &str, root_digest: &str, now: Instant) -> bool {
        self.0.engine_id == engine_id
            && self.0.docker_root_digest == root_digest
            && self.0.is_recent_at(now)
    }
}
