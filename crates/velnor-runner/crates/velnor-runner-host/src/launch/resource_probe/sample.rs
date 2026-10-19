//! Non-serializable results scoped to one completed probe transaction.

mod production_profile;
mod storage_admission;
mod witness;
pub(in crate::launch) use production_profile::retained_tmpfs_bytes;
pub(in crate::launch) use storage_admission::filesystem_instance_digest;
pub(crate) use witness::StartObservationWitness;
#[cfg(test)]
mod storage_admission_tests;

#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::worker::ResourceBudget;

use super::ProbeStartupEvidence;
use super::record::ProbeRecord;

const MAX_OBSERVATION_AGE: Duration = Duration::from_secs(30);

/// A validated selected-guest sample that cannot survive a process restart.
pub(super) struct Observation {
    record: ProbeRecord,
    cpus: u32,
    effective_cpu_millicores: u64,
    memory_total_bytes: u64,
    engine_id: String,
    docker_root_digest: String,
    sample_generation_id: String,
    observed_at: Instant,
    startup_evidence: ProbeStartupEvidence,
}

impl Observation {
    pub(super) fn new(
        record: ProbeRecord,
        cpus: u32,
        inspected_host_pid: u32,
        memory_total_bytes: u64,
        engine_id: String,
        docker_root_digest: String,
        sample_generation_id: String,
        observed_at: Instant,
        startup_evidence: ProbeStartupEvidence,
    ) -> Option<Self> {
        let effective_cpu_millicores = record.effective_cpu_millicores(cpus)?;
        (cpus > 0
            && inspected_host_pid > 0
            && record.probe_host_pid == inspected_host_pid
            && record.probe_namespace_pid == inspected_host_pid
            && memory_total_bytes > 0
            && record.memory_available_bytes <= memory_total_bytes
            && !engine_id.is_empty()
            && startup_evidence.cleanup().operation_id() == sample_generation_id
            && startup_evidence.cleanup().engine_id() == engine_id
            && startup_evidence.cleanup().docker_root_digest() == docker_root_digest
            && startup_evidence.cleanup().operation_name()
                == format!("velnor-resource-probe-{sample_generation_id}")
            && !startup_evidence.cleanup().container_id().is_empty()
            && valid_generation_id(&sample_generation_id))
        .then_some(Self {
            record,
            cpus,
            effective_cpu_millicores,
            memory_total_bytes,
            engine_id,
            docker_root_digest,
            sample_generation_id,
            observed_at,
            startup_evidence,
        })
    }

    #[cfg(test)]
    pub(super) fn test_new(
        record: ProbeRecord,
        cpus: u32,
        inspected_host_pid: u32,
        memory_total_bytes: u64,
        engine_id: String,
        docker_root_digest: String,
        sample_generation_id: String,
        observed_at: Instant,
    ) -> Option<Self> {
        let evidence = ProbeStartupEvidence::test_fixture(
            &sample_generation_id,
            &engine_id,
            &docker_root_digest,
        );
        Self::new(
            record,
            cpus,
            inspected_host_pid,
            memory_total_bytes,
            engine_id,
            docker_root_digest,
            sample_generation_id,
            observed_at,
            evidence,
        )
    }

    pub(super) fn startup_evidence(&self) -> &ProbeStartupEvidence {
        &self.startup_evidence
    }

    pub(super) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(super) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }

    pub(super) fn sample_generation_id(&self) -> &str {
        &self.sample_generation_id
    }

    pub(super) const fn memory_total_bytes(&self) -> u64 {
        self.memory_total_bytes
    }

    pub(super) fn is_recent_at(&self, now: Instant) -> bool {
        now.checked_duration_since(self.observed_at)
            .is_some_and(|age| age <= MAX_OBSERVATION_AGE)
    }

    #[cfg(test)]
    pub(super) fn observed_at(&self) -> Instant {
        self.observed_at
    }

    pub(super) fn doctor_report(&self) -> Option<serde_json::Value> {
        self.is_recent_at(Instant::now()).then(|| {
            serde_json::json!({
                "engine_id": &self.engine_id,
                "docker_root_digest": &self.docker_root_digest,
                "sample_generation_id": &self.sample_generation_id,
                "cpu_count": self.cpus,
                "cpu_millicores": self.effective_cpu_millicores,
                "memory_total_bytes": self.memory_total_bytes,
                "memory_available_bytes": self.record.memory_available_bytes,
                "docker_root_free_bytes": self.record.docker_root_free_bytes,
                "load_milli": self.record.load_milli,
            })
        })
    }

    pub(super) fn into_start_permit(self, budget: ResourceBudget) -> Option<StartPermit> {
        let pair = budget.pair();
        let reserve = budget.admission_reserve();
        let required_cpu = pair.cpu_millicores.checked_add(reserve.cpu_millicores)?;
        let required_memory = pair.memory_bytes.checked_add(reserve.memory_bytes)?;
        let recent = self.is_recent_at(Instant::now());
        (recent
            && self.record.docker_root_free_bytes >= 10 * 1024 * 1024 * 1024
            && self.effective_cpu_millicores >= required_cpu
            && self.record.memory_available_bytes >= required_memory)
            .then_some(StartPermit {
                observation: self,
                cpu_millicores: pair.cpu_millicores,
                memory_bytes: pair.memory_bytes,
                budget,
            })
    }
}

/// Single-use token for the start boundary; it is neither cloneable nor serializable.
pub(crate) struct StartPermit {
    observation: Observation,
    cpu_millicores: u64,
    memory_bytes: u64,
    budget: ResourceBudget,
}

impl StartPermit {
    pub(crate) const fn resource_budget(&self) -> ResourceBudget {
        self.budget
    }

    pub(crate) fn storage_profile(
        &self,
        images: &crate::launch::VerifiedWorkerImages,
        budget: ResourceBudget,
        max_jobs: u32,
    ) -> Option<crate::journal::StorageProfile> {
        production_profile::build(&self.observation, images, budget, max_jobs)
            .map(production_profile::StartupStorageProfile::into_profile)
    }

    pub(crate) fn storage_provisioning_admission(
        &self,
        profile: &crate::journal::StorageProfile,
        expected_engine_id: &str,
        expected_docker_root_digest: &str,
    ) -> Option<crate::journal::StorageProvisioningAdmission> {
        self.observation.storage_provisioning_admission(
            profile,
            expected_engine_id,
            expected_docker_root_digest,
        )
    }

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
        StartObservationWitness::from_observation(
            &self.observation.record,
            self.observation.engine_id.clone(),
            self.observation.docker_root_digest.clone(),
            self.observation.sample_generation_id.clone(),
            self.observation.observed_at,
        )
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
        let startup_evidence =
            ProbeStartupEvidence::test_fixture(generation, "test-engine", &docker_root_digest);
        StartPermit {
            observation: Observation {
                record: super::record::fixture_record(),
                cpus: 1,
                effective_cpu_millicores: 1_000,
                memory_total_bytes: 64 * 1024 * 1024 * 1024,
                engine_id: "test-engine".to_owned(),
                docker_root_digest,
                sample_generation_id: generation.to_owned(),
                observed_at: Instant::now(),
                startup_evidence,
            },
            cpu_millicores: 2_000,
            memory_bytes: 8 * 1024 * 1024 * 1024,
            budget: crate::worker::ResourceBudgetConfig {
                runner_cpu_millicores: 1_000,
                runner_memory_bytes: 4 * 1024 * 1024 * 1024,
                dind_cpu_millicores: 1_000,
                dind_memory_bytes: 4 * 1024 * 1024 * 1024,
                operational_memory_reserve_bytes: 1,
            }
            .validate()
            .expect("valid fixture budget"),
        }
    }

    #[cfg(test)]
    pub(crate) fn test_fixture_with_available_memory(
        generation: &str,
        available_memory_bytes: u64,
    ) -> Self {
        let mut permit = Self::test_fixture_with_generation(generation);
        assert!(available_memory_bytes <= permit.observation.memory_total_bytes);
        permit.observation.record.memory_available_bytes = available_memory_bytes;
        permit
    }

    #[cfg(test)]
    pub(crate) fn test_fixture_with_engine_and_available_memory(
        generation: &str,
        engine_id: &str,
        available_memory_bytes: u64,
    ) -> Self {
        let mut permit = Self::test_fixture_with_engine_id(generation, engine_id);
        assert!(available_memory_bytes <= permit.observation.memory_total_bytes);
        permit.observation.record.memory_available_bytes = available_memory_bytes;
        permit
    }

    #[cfg(test)]
    pub(crate) fn test_fixture_with_engine_id(generation: &str, engine_id: &str) -> Self {
        assert!(valid_generation_id(generation));
        assert!(!engine_id.is_empty() && engine_id.len() <= 128);
        let docker_root_digest = super::projection::DockerRoot::parse("/var/lib/docker")
            .map(|root| root.digest().to_owned())
            .unwrap_or_default();
        let startup_evidence =
            ProbeStartupEvidence::test_fixture(generation, engine_id, &docker_root_digest);
        StartPermit {
            observation: Observation {
                record: super::record::fixture_record(),
                cpus: 1,
                effective_cpu_millicores: 1_000,
                memory_total_bytes: 64 * 1024 * 1024 * 1024,
                engine_id: engine_id.to_owned(),
                docker_root_digest,
                sample_generation_id: generation.to_owned(),
                observed_at: Instant::now(),
                startup_evidence,
            },
            cpu_millicores: 2_000,
            memory_bytes: 8 * 1024 * 1024 * 1024,
            budget: crate::worker::ResourceBudgetConfig {
                runner_cpu_millicores: 1_000,
                runner_memory_bytes: 4 * 1024 * 1024 * 1024,
                dind_cpu_millicores: 1_000,
                dind_memory_bytes: 4 * 1024 * 1024 * 1024,
                operational_memory_reserve_bytes: 1,
            }
            .validate()
            .expect("valid fixture budget"),
        }
    }
}

fn valid_generation_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
