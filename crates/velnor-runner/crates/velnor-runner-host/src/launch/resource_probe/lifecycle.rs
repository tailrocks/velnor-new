//! One bounded probe attempt, including prior-operation recovery.

use std::future::Future;
use std::time::{Duration, Instant};

use bollard::{Docker, models::SystemInfo};
use uuid::Uuid;

use crate::docker_client::DOCKER_OPERATION_TIMEOUT;
use crate::error::HostError;
use crate::journal::{Journal, ProbePhase, ProbeSeed};

use super::projection::{DockerRoot, ProbeProjection, VerifiedProbeImage};
use super::provider::{ImageProvider, VerifiedCandidate};
use super::sample::Observation;
use super::{execute, recover};

const PROBE_TIMEOUT: Duration = Duration::from_secs(30);

pub(super) async fn collect<P: ImageProvider>(
    docker: &Docker,
    journal: &Journal,
    provider: &P,
) -> Result<Option<Observation>, HostError> {
    let deadline = Deadline::new();
    let mut active = deadline.within(journal.active_probe()).await??;
    if let Some(row) = active.as_ref()
        && row.phase == ProbePhase::Prepared
    {
        deadline
            .within(journal.abort_prepared_probe(&row.operation_id))
            .await??;
        active = None;
    }
    let before = info(docker, &deadline).await?;
    if let Some(row) = active.as_ref()
        && before.id.as_deref() != Some(row.engine_id.as_str())
    {
        return Err(HostError::Identity);
    }
    let (engine_id, root, cpu_count, memory_total) = bind_info(&before, journal, &deadline).await?;
    if let Some(row) = active {
        Box::pin(recover::active(
            docker, journal, &before, &root, &deadline, row,
        ))
        .await?;
    }
    let Some(candidate) = deadline
        .within(provider.verified_candidate(docker, &engine_id, &before))
        .await??
    else {
        return Ok(None);
    };
    let image = verified_image(candidate)?;
    let projection = projection(journal, &root, &engine_id, image, &deadline).await?;
    let output = execute::run(docker, journal, &projection, memory_total, &deadline).await?;
    let after = info(docker, &deadline).await?;
    if !same_guest(&after, &engine_id, &root)? {
        return Err(HostError::Identity);
    }
    Ok(observation(
        output,
        cpu_count,
        memory_total,
        engine_id,
        root.digest().to_owned(),
    ))
}

pub(super) fn observation(
    output: execute::ObservedProbeRecord,
    cpu_count: u32,
    memory_total: u64,
    engine_id: String,
    root_digest: String,
) -> Option<Observation> {
    Observation::new(
        output.record,
        cpu_count,
        memory_total,
        engine_id,
        root_digest,
        output.observed_at,
    )
}

fn verified_image(candidate: VerifiedCandidate) -> Result<VerifiedProbeImage, HostError> {
    VerifiedProbeImage::from_verified_provider(
        candidate.runtime_id,
        candidate.platform,
        candidate.source_revision,
        candidate.binding_fingerprint,
        &candidate.config,
    )
}

async fn info(docker: &Docker, deadline: &Deadline) -> Result<SystemInfo, HostError> {
    deadline
        .docker(docker.info())
        .await?
        .map_err(|_| HostError::Docker)
}

async fn bind_info(
    info: &SystemInfo,
    journal: &Journal,
    deadline: &Deadline,
) -> Result<(String, DockerRoot, u32, u64), HostError> {
    let engine_id = info
        .id
        .clone()
        .filter(|id| !id.trim().is_empty())
        .ok_or(HostError::Identity)?;
    let bound = deadline.within(journal.engine_id()).await??;
    let root = DockerRoot::parse(info.docker_root_dir.as_deref().ok_or(HostError::Path)?)?;
    let cpu_count = u32::try_from(info.ncpu.ok_or(HostError::Docker)?)
        .ok()
        .filter(|value| *value > 0)
        .ok_or(HostError::Docker)?;
    let memory_total = u64::try_from(info.mem_total.ok_or(HostError::Docker)?)
        .ok()
        .filter(|value| *value > 0)
        .ok_or(HostError::Docker)?;
    if engine_id != bound {
        return Err(HostError::Identity);
    }
    Ok((engine_id, root, cpu_count, memory_total))
}

async fn projection(
    journal: &Journal,
    root: &DockerRoot,
    engine_id: &str,
    image: VerifiedProbeImage,
    deadline: &Deadline,
) -> Result<ProbeProjection, HostError> {
    let operation_id = Uuid::new_v4().simple().to_string();
    let instance_id = deadline.within(journal.instance_id()).await??;
    let root_path = root.path();
    let projection = ProbeProjection::build(
        operation_id,
        instance_id.clone(),
        engine_id.to_owned(),
        root_path,
        image,
    )?;
    let seed = ProbeSeed {
        operation_id: projection.operation_id.clone(),
        instance_id,
        engine_id: projection.engine_id.clone(),
        docker_root_digest: projection.root.digest().to_owned(),
        source_revision: projection.image.source_revision().to_owned(),
        runtime_image_id: projection.image.runtime_id().to_owned(),
        image_binding_digest: projection.image.binding_fingerprint().to_owned(),
        operation_name: projection.name.clone(),
        projection_digest: projection.projection_digest.clone(),
    };
    deadline.within(journal.prepare_probe(seed)).await??;
    Ok(projection)
}

fn same_guest(info: &SystemInfo, engine_id: &str, root: &DockerRoot) -> Result<bool, HostError> {
    let current_id = info.id.as_deref().ok_or(HostError::Identity)?;
    let current_root = DockerRoot::parse(info.docker_root_dir.as_deref().ok_or(HostError::Path)?)?;
    Ok(current_id == engine_id && current_root.digest() == root.digest())
}

pub(super) struct Deadline {
    began: Instant,
    timeout: Duration,
    docker_timeout: Duration,
}

impl Deadline {
    pub(super) fn new() -> Self {
        Self {
            began: Instant::now(),
            timeout: PROBE_TIMEOUT,
            docker_timeout: DOCKER_OPERATION_TIMEOUT,
        }
    }

    #[cfg(test)]
    pub(super) fn test_with_timeouts(timeout: Duration, docker_timeout: Duration) -> Self {
        Self {
            began: Instant::now(),
            timeout,
            docker_timeout,
        }
    }

    pub(super) fn remaining(&self) -> Duration {
        self.timeout.saturating_sub(self.began.elapsed())
    }

    pub(super) fn ensure_remaining(&self) -> Result<(), HostError> {
        (!self.remaining().is_zero())
            .then_some(())
            .ok_or(HostError::DockerTimeout)
    }

    pub(super) async fn within<F: Future>(&self, future: F) -> Result<F::Output, HostError> {
        self.ensure_remaining()?;
        tokio::time::timeout(self.remaining(), future)
            .await
            .map_err(|_| HostError::DockerTimeout)
    }

    pub(super) async fn docker<F: Future>(&self, future: F) -> Result<F::Output, HostError> {
        self.ensure_remaining()?;
        let timeout = self.remaining().min(self.docker_timeout);
        tokio::time::timeout(timeout, future)
            .await
            .map_err(|_| HostError::DockerTimeout)
    }
}
