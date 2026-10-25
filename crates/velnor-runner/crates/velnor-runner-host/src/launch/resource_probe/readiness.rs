//! Run probe operations one at a time and collect bounded readiness observations.

use std::time::{Duration, Instant};

use bollard::Docker;

use crate::{
    error::HostError,
    journal::Journal,
    launch::{ReleaseReadSecret, SelectedEngineContext},
};

use super::{
    lifecycle,
    provider::ReleaseImageProvider,
    sample::{Observation, pressure::FreshPressureSample},
};

const MAX_OBSERVATION_AGE: Duration = Duration::from_secs(30);

/// Allow only one probe owner in this process.
/// The journal's probe leases remain authoritative.
pub(super) async fn lock_operation(
    journal: &Journal,
) -> Result<tokio::sync::OwnedMutexGuard<()>, HostError> {
    journal.probe_execution_lock().await
}

/// A readiness sampler must leave any existing probe owner untouched.
pub(super) fn try_lock_operation(
    journal: &Journal,
) -> Result<Option<tokio::sync::OwnedMutexGuard<()>>, HostError> {
    journal.try_probe_execution_lock()
}

/// This sample contains current facts from a capacity probe on the selected engine after cleanup.
#[derive(Clone, Debug)]
pub(crate) struct ReadinessSample {
    engine_id: String,
    docker_root_digest: String,
    generation: String,
    observed_at: Instant,
    effective_cpu_millicores: u64,
    memory_total_bytes: u64,
    memory_available_bytes: u64,
    pressure_sample: Option<FreshPressureSample>,
}

impl ReadinessSample {
    pub(crate) fn matches_engine(&self, selected: &SelectedEngineContext) -> bool {
        self.engine_id == selected.engine_id()
            && self.docker_root_digest == selected.docker_root_digest()
    }

    pub(crate) fn is_recent(&self, now: Instant) -> bool {
        now.checked_duration_since(self.observed_at)
            .is_some_and(|age| age <= MAX_OBSERVATION_AGE)
    }

    pub(crate) fn generation(&self) -> &str {
        &self.generation
    }

    pub(crate) const fn observed_at(&self) -> Instant {
        self.observed_at
    }

    pub(crate) const fn effective_cpu_millicores(&self) -> u64 {
        self.effective_cpu_millicores
    }

    pub(crate) const fn memory_total_bytes(&self) -> u64 {
        self.memory_total_bytes
    }

    pub(crate) const fn memory_available_bytes(&self) -> u64 {
        self.memory_available_bytes
    }

    pub(crate) const fn pressure_sample(&self) -> Option<FreshPressureSample> {
        self.pressure_sample
    }
}

impl From<Observation> for ReadinessSample {
    fn from(observation: Observation) -> Self {
        let pressure_sample = observation.pressure_sample_at(Instant::now());
        Self {
            engine_id: observation.engine_id().to_owned(),
            docker_root_digest: observation.docker_root_digest().to_owned(),
            generation: observation.sample_generation_id().to_owned(),
            observed_at: observation.observed_at(),
            effective_cpu_millicores: observation.effective_cpu_millicores(),
            memory_total_bytes: observation.memory_total_bytes(),
            memory_available_bytes: observation.memory_available_bytes(),
            pressure_sample,
        }
    }
}

/// Run the existing capacity-probe lifecycle without recovering another operation.
pub(crate) async fn collect(
    docker: &Docker,
    journal: &Journal,
    secret: &ReleaseReadSecret,
) -> Result<Option<ReadinessSample>, HostError> {
    let provider_token = secret.as_provider_token()?;
    let provider = ReleaseImageProvider::from_compiled_source(provider_token, journal)
        .map_err(|_| HostError::Docker)?;
    Box::pin(collect_with_provider(docker, journal, &provider)).await
}

pub(crate) async fn collect_with_provider<P: super::provider::ImageProvider>(
    docker: &Docker,
    journal: &Journal,
    provider: &P,
) -> Result<Option<ReadinessSample>, HostError> {
    let Some(_operation) = try_lock_operation(journal)? else {
        return Ok(None);
    };
    if journal.active_probe().await?.is_some() {
        return Ok(None);
    }
    Ok(lifecycle::collect_for_readiness(docker, journal, provider)
        .await?
        .map(ReadinessSample::from))
}

#[cfg(test)]
pub(crate) async fn collect_with_provider_and_operation_id<P: super::provider::ImageProvider>(
    docker: &Docker,
    journal: &Journal,
    provider: &P,
    operation_id: &str,
) -> Result<Option<ReadinessSample>, HostError> {
    let Some(_operation) = try_lock_operation(journal)? else {
        return Ok(None);
    };
    if journal.active_probe().await?.is_some() {
        return Ok(None);
    }
    Ok(
        lifecycle::collect_for_readiness_with_operation_id(docker, journal, provider, operation_id)
            .await?
            .map(ReadinessSample::from),
    )
}
