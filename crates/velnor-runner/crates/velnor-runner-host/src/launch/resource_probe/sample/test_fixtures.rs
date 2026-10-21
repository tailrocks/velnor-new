use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use super::{Observation, StartPermit, valid_generation_id};

impl StartPermit {
    pub(in crate::launch::resource_probe) fn test_is_valid_at(&self, now: Instant) -> bool {
        self.is_valid_at("test-engine", &self.observation.docker_root_digest, now)
    }

    pub(crate) fn test_fixture() -> Self {
        static NEXT_GENERATION: AtomicU64 = AtomicU64::new(0);
        let generation = NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
        Self::test_fixture_with_generation(&format!("{generation:032x}"))
    }

    pub(crate) fn test_fixture_with_generation(generation: &str) -> Self {
        fixture(generation, "test-engine")
    }

    pub(crate) fn test_fixture_with_available_memory(
        generation: &str,
        available_memory_bytes: u64,
    ) -> Self {
        let mut permit = Self::test_fixture_with_generation(generation);
        assert!(available_memory_bytes <= permit.observation.memory_total_bytes);
        permit.observation.record.memory_available_bytes = available_memory_bytes;
        permit
    }

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

    pub(crate) fn test_fixture_with_engine_id(generation: &str, engine_id: &str) -> Self {
        fixture(generation, engine_id)
    }

    pub(crate) fn test_fixture_with_guest_capacity(
        generation: &str,
        engine_id: &str,
        cpu_millicores: u64,
        memory_total_bytes: u64,
        memory_available_bytes: u64,
    ) -> Self {
        fixture_with_guest_capacity(
            generation,
            engine_id,
            cpu_millicores,
            memory_total_bytes,
            memory_available_bytes,
        )
    }
}

fn fixture(generation: &str, engine_id: &str) -> StartPermit {
    // These admission tests configure no more than 16,000 CPU millicores and
    // 128 GiB of worker memory. Tests with insufficient capacity use smaller samples.
    fixture_with_guest_capacity(
        generation,
        engine_id,
        20_000,
        256 * 1024 * 1024 * 1024,
        256 * 1024 * 1024 * 1024,
    )
}

fn fixture_with_guest_capacity(
    generation: &str,
    engine_id: &str,
    cpu_millicores: u64,
    memory_total_bytes: u64,
    memory_available_bytes: u64,
) -> StartPermit {
    assert!(valid_generation_id(generation));
    assert!(!engine_id.is_empty() && engine_id.len() <= 128);
    assert!(cpu_millicores > 0);
    assert!(memory_total_bytes > 0);
    assert!(memory_available_bytes <= memory_total_bytes);

    let cpus = u32::try_from(cpu_millicores.div_ceil(1_000))
        .expect("The CPU count in the test fixture is too large.");
    assert!(cpus <= 4096);
    let root_quota_micros = cpu_millicores
        .checked_mul(100)
        .expect("The CPU limit in the test fixture is too large.");
    let docker_root_digest = super::super::projection::DockerRoot::parse("/var/lib/docker")
        .map(|root| root.digest().to_owned())
        .unwrap_or_default();
    let mut record = super::super::record::fixture_record();
    record.memory_available_bytes = memory_available_bytes;
    record.probe_cgroup_cpuset_cpus_count = cpus;
    for ancestor in &mut record.cpu_ancestor_limits {
        ancestor.cpuset_cpus_count = cpus;
    }
    record.cpu_ancestor_limits[0].cpu_quota_micros = Some(root_quota_micros);
    let observation = Observation::test_new(
        record,
        cpus,
        1234,
        memory_total_bytes,
        engine_id.to_owned(),
        docker_root_digest,
        generation.to_owned(),
        Instant::now(),
    )
    .expect("The observation in the test fixture does not match the probe record.");
    StartPermit {
        observation,
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
mod tests {
    use super::*;

    #[test]
    fn guest_capacity_fixtures_derive_cpu_from_probe_record() {
        let default = StartPermit::test_fixture_with_generation(&"a".repeat(32));
        assert_eq!(default.observation.effective_cpu_millicores, 20_000);
        assert_eq!(default.observation.cpus, 20);
        assert_eq!(
            default
                .observation
                .record
                .effective_cpu_millicores(default.observation.cpus),
            Some(default.observation.effective_cpu_millicores)
        );

        let fractional = StartPermit::test_fixture_with_guest_capacity(
            &"b".repeat(32),
            "test-engine",
            20_001,
            256 * 1024 * 1024 * 1024,
            255 * 1024 * 1024 * 1024,
        );
        assert_eq!(fractional.observation.effective_cpu_millicores, 20_001);
        assert_eq!(fractional.observation.cpus, 21);
        assert_eq!(
            fractional
                .observation
                .record
                .effective_cpu_millicores(fractional.observation.cpus),
            Some(fractional.observation.effective_cpu_millicores)
        );

        let configured = StartPermit::test_fixture_with_guest_capacity(
            &"c".repeat(32),
            "test-engine",
            61_000,
            256 * 1024 * 1024 * 1024,
            255 * 1024 * 1024 * 1024,
        );
        assert_eq!(configured.observation.effective_cpu_millicores, 61_000);
        assert_eq!(configured.observation.cpus, 61);
        assert_eq!(
            configured
                .observation
                .record
                .effective_cpu_millicores(configured.observation.cpus),
            Some(configured.observation.effective_cpu_millicores)
        );
    }
}
