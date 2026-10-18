//! Non-serializable results scoped to one completed probe transaction.

#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::launch::pressure::Sample;
use crate::worker::ResourceBudget;

use super::record::ProbeRecord;

const MAX_OBSERVATION_AGE: Duration = Duration::from_secs(30);

/// A validated selected-guest sample that cannot survive a process restart.
pub(super) struct Observation {
    record: ProbeRecord,
    cpus: u32,
    engine_id: String,
    docker_root_digest: String,
    sample_generation_id: String,
    observed_at: Instant,
}

impl Observation {
    pub(super) fn new(
        record: ProbeRecord,
        cpus: u32,
        memory_total_bytes: u64,
        engine_id: String,
        docker_root_digest: String,
        sample_generation_id: String,
        observed_at: Instant,
    ) -> Option<Self> {
        (cpus > 0
            && memory_total_bytes > 0
            && record.memory_available_bytes <= memory_total_bytes
            && !engine_id.is_empty()
            && valid_generation_id(&sample_generation_id))
        .then_some(Self {
            record,
            cpus,
            engine_id,
            docker_root_digest,
            sample_generation_id,
            observed_at,
        })
    }

    pub(super) fn is_recent_at(&self, now: Instant) -> bool {
        now.checked_duration_since(self.observed_at)
            .is_some_and(|age| age <= MAX_OBSERVATION_AGE)
    }

    #[cfg(test)]
    pub(super) fn observed_at(&self) -> Instant {
        self.observed_at
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
        let reserve = budget.admission_reserve();
        let required_cpu = pair.cpu_millicores.checked_add(reserve.cpu_millicores)?;
        let required_memory = pair.memory_bytes.checked_add(reserve.memory_bytes)?;
        let available_cpu = super::projection::total_cpu_millicores(self.cpus)?;
        let recent = self.is_recent_at(Instant::now());
        (recent
            && self.record.docker_root_free_bytes >= 10 * 1024 * 1024 * 1024
            && available_cpu >= required_cpu
            && self.record.memory_available_bytes >= required_memory)
            .then_some(StartPermit {
                observation: self,
                cpu_millicores: pair.cpu_millicores,
                memory_bytes: pair.memory_bytes,
            })
    }
}

/// Single-use token for the start boundary; it is neither cloneable nor serializable.
pub(crate) struct StartPermit {
    observation: Observation,
    cpu_millicores: u64,
    memory_bytes: u64,
}

impl StartPermit {
    pub(super) fn consume(self, current_engine_id: &str, current_root_digest: &str) -> bool {
        self.is_valid_at(current_engine_id, current_root_digest, Instant::now())
    }

    /// Consume the one-use sample at the durable admission boundary.
    pub(crate) fn consume_for_admission(
        self,
        current_engine_id: &str,
        current_root_digest: &str,
    ) -> bool {
        self.consume(current_engine_id, current_root_digest)
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.observation.engine_id
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.observation.docker_root_digest
    }

    pub(crate) fn sample_generation_id(&self) -> &str {
        &self.observation.sample_generation_id
    }

    /// Preserve this sample's identity and freshness for effects after admission.
    #[must_use]
    pub(crate) fn observation_witness(&self) -> StartObservationWitness {
        StartObservationWitness {
            engine_id: self.observation.engine_id.clone(),
            docker_root_digest: self.observation.docker_root_digest.clone(),
            sample_generation_id: self.observation.sample_generation_id.clone(),
            observed_at: self.observation.observed_at,
        }
    }

    #[cfg(test)]
    pub(crate) fn stale_observation_witness(&self) -> StartObservationWitness {
        let mut witness = self.observation_witness();
        witness.observed_at -= MAX_OBSERVATION_AGE + Duration::from_millis(1);
        witness
    }

    pub(crate) const fn charge(&self) -> (u64, u64) {
        (self.cpu_millicores, self.memory_bytes)
    }

    fn is_valid_at(&self, engine_id: &str, root_digest: &str, now: Instant) -> bool {
        self.observation.engine_id == engine_id
            && self.observation.docker_root_digest == root_digest
            && self.observation.is_recent_at(now)
    }

    #[cfg(test)]
    pub(super) fn test_is_valid_at(&self, now: Instant) -> bool {
        self.is_valid_at("test-engine", &self.observation.docker_root_digest, now)
    }

    #[cfg(test)]
    pub(crate) fn test_fixture() -> Self {
        static NEXT_GENERATION: AtomicU64 = AtomicU64::new(0);
        let generation = NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
        Self::test_fixture_with_generation(&format!("{generation:032x}"))
    }

    #[cfg(test)]
    pub(crate) fn test_fixture_with_generation(generation: &str) -> Self {
        assert!(valid_generation_id(generation));
        let docker_root_digest = super::projection::DockerRoot::parse("/var/lib/docker")
            .map(|root| root.digest().to_owned())
            .unwrap_or_default();
        StartPermit {
            observation: Observation {
                record: ProbeRecord {
                    schema_version: 1,
                    docker_root_free_bytes: 20 * 1024 * 1024 * 1024,
                    docker_root_total_bytes: 40 * 1024 * 1024 * 1024,
                    memory_available_bytes: 8 * 1024 * 1024 * 1024,
                    load_milli: 125,
                    memory_psi_some_avg10_bps: None,
                },
                cpus: 1,
                engine_id: "test-engine".to_owned(),
                docker_root_digest,
                sample_generation_id: generation.to_owned(),
                observed_at: Instant::now(),
            },
            cpu_millicores: 2_000,
            memory_bytes: 8 * 1024 * 1024 * 1024,
        }
    }
}

/// Freshness evidence for an already charged reservation; it cannot reserve capacity.
pub(crate) struct StartObservationWitness {
    engine_id: String,
    docker_root_digest: String,
    sample_generation_id: String,
    observed_at: Instant,
}

impl StartObservationWitness {
    /// Whether the original sample remains fresh for this exact engine and Docker root.
    #[must_use]
    pub(crate) fn is_fresh_for(
        &self,
        engine_id: &str,
        docker_root_digest: &str,
        sample_generation_id: &str,
    ) -> bool {
        self.engine_id == engine_id
            && self.docker_root_digest == docker_root_digest
            && self.sample_generation_id == sample_generation_id
            && self.observed_at.elapsed() <= MAX_OBSERVATION_AGE
    }

    #[cfg(test)]
    fn is_fresh_at(
        &self,
        engine_id: &str,
        root_digest: &str,
        generation_id: &str,
        now: Instant,
    ) -> bool {
        self.engine_id == engine_id
            && self.docker_root_digest == root_digest
            && self.sample_generation_id == generation_id
            && now
                .checked_duration_since(self.observed_at)
                .is_some_and(|age| age <= MAX_OBSERVATION_AGE)
    }
}

#[cfg(test)]
mod witness_tests {
    use std::time::{Duration, Instant};

    use super::StartPermit;

    #[test]
    fn observation_witness_is_bound_to_one_sample_and_selected_engine() -> Result<(), String> {
        let permit = StartPermit::test_fixture_with_generation(&"a".repeat(32));
        let root = permit.docker_root_digest().to_owned();
        let witness = permit.observation_witness();
        assert!(witness.is_fresh_for("test-engine", &root, &"a".repeat(32)));
        assert!(!witness.is_fresh_for("other-engine", &root, &"a".repeat(32)));
        assert!(!witness.is_fresh_for("test-engine", &"b".repeat(64), &"a".repeat(32)));
        assert!(!witness.is_fresh_for("test-engine", &root, &"c".repeat(32)));
        Ok(())
    }

    #[test]
    fn observation_witness_expires_without_authorizing_a_new_reservation() -> Result<(), String> {
        let permit = StartPermit::test_fixture_with_generation(&"d".repeat(32));
        let root = permit.docker_root_digest().to_owned();
        let witness = permit.observation_witness();
        let stale = witness
            .observed_at
            .checked_add(Duration::from_millis(30_001))
            .ok_or_else(|| "could not construct expiry instant".to_owned())?;
        assert!(!witness.is_fresh_at("test-engine", &root, &"d".repeat(32), stale));
        assert!(!witness.is_fresh_at(
            "test-engine",
            &root,
            &"d".repeat(32),
            Instant::now() - Duration::from_millis(30_001)
        ));
        Ok(())
    }
}

fn valid_generation_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
