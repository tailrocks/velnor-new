// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

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

pub(super) async fn lock_operation(
    journal: &Journal,
) -> Result<tokio::sync::OwnedMutexGuard<()>, HostError> {
    journal.probe_execution_lock().await
}

pub(super) fn try_lock_operation(
    journal: &Journal,
) -> Result<Option<tokio::sync::OwnedMutexGuard<()>>, HostError> {
    journal.try_probe_execution_lock()
}

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

pub(crate) async fn collect(
    docker: &Docker,
    journal: &Journal,
    secret: &ReleaseReadSecret,
    endpoint: &str,
) -> Result<Option<ReadinessSample>, HostError> {
    let provider_token = secret.as_provider_token()?;
    let provider = ReleaseImageProvider::from_compiled_source(provider_token, journal, endpoint)
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
