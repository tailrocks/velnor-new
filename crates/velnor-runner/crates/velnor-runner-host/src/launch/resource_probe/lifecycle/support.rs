use std::future::Future;
use std::time::{Duration, Instant};

use bollard::{Docker, models::SystemInfo};
use uuid::Uuid;

use crate::docker_client::DOCKER_OPERATION_TIMEOUT;
use crate::error::HostError;
use crate::journal::{
    DoctorProbePrepareResult, DoctorProbeTicket, Journal, ProbeSeed, ProbeWork,
    ResourceProbePurpose,
};

use super::super::execute;
use super::super::projection::{DockerRoot, ProbeProjection, VerifiedProbeImage};
use super::super::provider::{
    VerifiedCandidate, daemon_architecture as provider_daemon_architecture, daemon_platform_matches,
};
use super::super::sample::{Observation, ObservationInput};
use super::PROBE_TIMEOUT;

pub(in crate::launch::resource_probe) fn observation(
    output: execute::ObservedProbeRecord,
    cpu_count: u32,
    memory_total: u64,
    engine_id: String,
    root_digest: String,
) -> Option<Observation> {
    Observation::new(ObservationInput {
        record: output.record,
        cpus: cpu_count,
        inspected_host_pid: output.container_host_pid,
        memory_total_bytes: memory_total,
        engine_id,
        docker_root_digest: root_digest,
        sample_generation_id: output.sample_generation_id,
        observed_at: output.observed_at,
        startup_evidence: output.startup_evidence?,
    })
}

pub(super) fn verified_image(
    candidate: VerifiedCandidate,
) -> Result<VerifiedProbeImage, HostError> {
    VerifiedProbeImage::from_verified_provider(
        candidate.runtime_id,
        candidate.platform,
        candidate.daemon_architecture,
        candidate.source_revision,
        candidate.binding_fingerprint,
        &candidate.config,
    )
}

pub(in crate::launch::resource_probe) async fn info(
    docker: &Docker,
    deadline: &Deadline,
) -> Result<SystemInfo, HostError> {
    deadline
        .docker(docker.info())
        .await?
        .map_err(|_| HostError::Docker)
}

pub(in crate::launch::resource_probe) async fn bind_info(
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
    let (cpu_count, memory_total) = guest_capacity(info)?;
    if engine_id != bound {
        return Err(HostError::Identity);
    }
    Ok((engine_id, root, cpu_count, memory_total))
}

pub(in crate::launch::resource_probe) fn guest_capacity(
    info: &SystemInfo,
) -> Result<(u32, u64), HostError> {
    let cpu_count = u32::try_from(info.ncpu.ok_or(HostError::Docker)?)
        .ok()
        .filter(|value| *value > 0)
        .ok_or(HostError::Docker)?;
    let memory_total = u64::try_from(info.mem_total.ok_or(HostError::Docker)?)
        .ok()
        .filter(|value| *value > 0)
        .ok_or(HostError::Docker)?;
    Ok((cpu_count, memory_total))
}

pub(super) async fn projection(
    journal: &Journal,
    root: &DockerRoot,
    engine_id: &str,
    image: VerifiedProbeImage,
    deadline: &Deadline,
    doctor_ticket: Option<&DoctorProbeTicket>,
    operation_id: Option<&str>,
) -> Result<ProbeProjection, HostError> {
    let operation_id =
        operation_id.map_or_else(|| Uuid::new_v4().simple().to_string(), str::to_owned);
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
        purpose: ResourceProbePurpose::Capacity,
        request_sha256: projection.request_sha256.clone(),
    };
    match doctor_ticket {
        Some(ticket) => {
            let child = match deadline
                .within(journal.prepare_probe_for_doctor_request(ticket, seed))
                .await??
            {
                DoctorProbePrepareResult::Prepared(child)
                | DoctorProbePrepareResult::AlreadyLinked(child) => child,
                DoctorProbePrepareResult::WaitingForProbe => return Err(HostError::Journal),
            };
            let ProbeWork::ResourceProbe {
                purpose: ResourceProbePurpose::Capacity,
                request_sha256,
                request_context: None,
            } = child.work()
            else {
                return Err(HostError::Journal);
            };
            if !doctor_child_identity_matches(
                &child.operation_id,
                request_sha256,
                &projection.operation_id,
                &projection.request_sha256,
            ) {
                return Err(HostError::Journal);
            }
        }
        None => deadline.within(journal.prepare_probe(seed)).await??,
    }
    Ok(projection)
}

fn doctor_child_identity_matches(
    child_operation_id: &str,
    child_request_sha256: &str,
    projection_operation_id: &str,
    projection_request_sha256: &str,
) -> bool {
    child_operation_id == projection_operation_id
        && child_request_sha256 == projection_request_sha256
}

pub(in crate::launch::resource_probe) fn same_guest(
    info: &SystemInfo,
    engine_id: &str,
    root: &DockerRoot,
    daemon_architecture: &str,
) -> Result<bool, HostError> {
    let current_id = info.id.as_deref().ok_or(HostError::Identity)?;
    let current_root = DockerRoot::parse(info.docker_root_dir.as_deref().ok_or(HostError::Path)?)?;
    Ok(daemon_platform_matches(info)
        && provider_daemon_architecture(info) == Some(daemon_architecture)
        && current_id == engine_id
        && current_root.digest() == root.digest())
}

pub(in crate::launch::resource_probe) struct Deadline {
    began: Instant,
    timeout: Duration,
    docker_timeout: Duration,
}

impl Deadline {
    pub(in crate::launch::resource_probe) fn new() -> Self {
        Self {
            began: Instant::now(),
            timeout: PROBE_TIMEOUT,
            docker_timeout: DOCKER_OPERATION_TIMEOUT,
        }
    }

    pub(in crate::launch::resource_probe) fn until(deadline: Instant) -> Self {
        let now = Instant::now();
        Self {
            began: now,
            timeout: deadline.saturating_duration_since(now),
            docker_timeout: DOCKER_OPERATION_TIMEOUT,
        }
    }

    #[cfg(test)]
    pub(in crate::launch::resource_probe) fn test_with_timeouts(
        timeout: Duration,
        docker_timeout: Duration,
    ) -> Self {
        Self {
            began: Instant::now(),
            timeout,
            docker_timeout,
        }
    }

    pub(in crate::launch::resource_probe) fn remaining(&self) -> Duration {
        self.timeout.saturating_sub(self.began.elapsed())
    }

    pub(in crate::launch::resource_probe) fn ensure_remaining(&self) -> Result<(), HostError> {
        (!self.remaining().is_zero())
            .then_some(())
            .ok_or(HostError::DockerTimeout)
    }

    pub(in crate::launch::resource_probe) async fn within<F: Future>(
        &self,
        future: F,
    ) -> Result<F::Output, HostError> {
        self.ensure_remaining()?;
        tokio::time::timeout(self.remaining(), future)
            .await
            .map_err(|_| HostError::DockerTimeout)
    }

    pub(in crate::launch::resource_probe) async fn docker<F: Future>(
        &self,
        future: F,
    ) -> Result<F::Output, HostError> {
        self.ensure_remaining()?;
        let timeout = self.remaining().min(self.docker_timeout);
        tokio::time::timeout(timeout, future)
            .await
            .map_err(|_| HostError::DockerTimeout)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn doctor_child_identity_requires_the_exact_operation_and_request_digest() {
        let operation_id = "a".repeat(32);
        let request_sha256 = "b".repeat(64);
        assert!(super::doctor_child_identity_matches(
            &operation_id,
            &request_sha256,
            &operation_id,
            &request_sha256,
        ));
        assert!(!super::doctor_child_identity_matches(
            &"c".repeat(32),
            &request_sha256,
            &operation_id,
            &request_sha256,
        ));
        assert!(!super::doctor_child_identity_matches(
            &operation_id,
            &"d".repeat(64),
            &operation_id,
            &request_sha256,
        ));
    }
}
