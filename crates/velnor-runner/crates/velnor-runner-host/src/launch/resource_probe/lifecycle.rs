//! One bounded probe attempt, including prior-operation recovery.

use std::future::Future;
use std::time::{Duration, Instant};

use bollard::{Docker, models::SystemInfo};
use uuid::Uuid;

use crate::docker_client::DOCKER_OPERATION_TIMEOUT;
use crate::error::HostError;
use crate::journal::{
    DoctorProbePrepareResult, DoctorProbeTicket, Journal, ProbePhase, ProbeSeed,
    ResourceProbePurpose,
};

use super::projection::{DockerRoot, ProbeProjection, VerifiedProbeImage};
use super::provider::{ImageProvider, VerifiedCandidate};
use super::sample::Observation;
use super::{execute, recover};

const PROBE_TIMEOUT: Duration = Duration::from_secs(30);

#[path = "lifecycle/publication_image.rs"]
mod publication_image;
pub(super) use publication_image::{verify_publication_image, verify_publication_image_until};

pub(super) async fn collect<P: ImageProvider>(
    docker: &Docker,
    journal: &Journal,
    provider: &P,
) -> Result<Option<Observation>, HostError> {
    collect_for_request(docker, journal, provider, None).await
}

pub(super) async fn collect_for_doctor<P: ImageProvider>(
    docker: &Docker,
    journal: &Journal,
    provider: &P,
    ticket: &DoctorProbeTicket,
) -> Result<Option<Observation>, HostError> {
    collect_for_request(docker, journal, provider, Some(ticket)).await
}

pub(super) async fn collect_owned_cgroups<P: ImageProvider>(
    docker: &Docker,
    journal: &Journal,
    provider: &P,
    subjects: &super::owned_cgroups::OwnedCgroupSubjects,
) -> Result<Option<super::owned_cgroups::VerifiedOwnedCgroups>, HostError> {
    let initial = Deadline::new();
    let active = initial.within(journal.active_probe()).await??;
    let before = info(docker, &initial).await?;
    let (engine_id, root, _, _) = bind_info(&before, journal, &initial).await?;
    if let Some(row) = active {
        recover::active(docker, journal, &before, &root, &initial, row).await?;
    }
    let Some(candidate) = provider
        .verified_candidate(docker, journal, &engine_id, &before)
        .await?
    else {
        return Ok(None);
    };
    let daemon_architecture = super::provider::daemon_architecture(&before)
        .ok_or(HostError::Identity)?
        .to_owned();
    let deadline = Deadline::new();
    let selected = info(docker, &deadline).await?;
    if !same_guest(&selected, &engine_id, &root, &daemon_architecture)? {
        return Err(HostError::Identity);
    }
    let operation_id = Uuid::new_v4().simple().to_string();
    let instance_id = deadline.within(journal.instance_id()).await??;
    let request_json = subjects.request_json(&operation_id, root.path())?;
    let request_sha256 = subjects.request_sha256(&operation_id, root.path())?;
    let image = verified_image(candidate)?;
    let projection = ProbeProjection::build_owned_cgroups(
        operation_id,
        instance_id.clone(),
        engine_id.clone(),
        root.path(),
        image,
        request_sha256.clone(),
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
        purpose: ResourceProbePurpose::OwnedCgroups,
        request_sha256,
    };
    deadline
        .within(journal.prepare_owned_cgroups_probe(seed, &request_json))
        .await??;
    let (observation, cleanup, probe_host_pid) =
        execute::run_owned_cgroups(docker, journal, &projection, subjects, &deadline).await?;
    let after = info(docker, &deadline).await?;
    if !same_guest(&after, &engine_id, &root, &daemon_architecture)? {
        return Err(HostError::Identity);
    }
    Ok(Some(super::owned_cgroups::VerifiedOwnedCgroups::new(
        observation,
        cleanup,
        probe_host_pid,
    )))
}

pub(super) async fn recover_doctor_request(
    docker: &Docker,
    journal: &Journal,
    ticket: &DoctorProbeTicket,
) -> Result<(), HostError> {
    let row = journal.begin_doctor_probe_recovery(ticket).await?;
    let deadline = Deadline::new();
    let current = info(docker, &deadline).await?;
    let (engine_id, root, _, _) = bind_info(&current, journal, &deadline).await?;
    if engine_id != row.engine_id || root.digest() != row.docker_root_digest {
        return Err(HostError::Identity);
    }
    recover::active(docker, journal, &current, &root, &deadline, row.clone()).await?;
    journal
        .finish_doctor_probe_request(
            ticket,
            &row.operation_id,
            crate::journal::DoctorProbeOutcome::Stale,
        )
        .await
}

async fn collect_for_request<P: ImageProvider>(
    docker: &Docker,
    journal: &Journal,
    provider: &P,
    doctor_ticket: Option<&DoctorProbeTicket>,
) -> Result<Option<Observation>, HostError> {
    let preparation_deadline = Deadline::new();
    let mut active = preparation_deadline
        .within(journal.active_probe())
        .await??;
    if let Some(row) = active.as_ref()
        && row.phase == ProbePhase::Prepared
    {
        preparation_deadline
            .within(journal.abort_prepared_probe(&row.operation_id))
            .await??;
        active = None;
    }
    let before = info(docker, &preparation_deadline).await?;
    if let Some(row) = active.as_ref()
        && before.id.as_deref() != Some(row.engine_id.as_str())
    {
        return Err(HostError::Identity);
    }
    let (engine_id, root, _, _) = bind_info(&before, journal, &preparation_deadline).await?;
    if let Some(row) = active {
        Box::pin(recover::active(
            docker,
            journal,
            &before,
            &root,
            &preparation_deadline,
            row,
        ))
        .await?;
    }
    let Some(candidate) = provider
        .verified_candidate(docker, journal, &engine_id, &before)
        .await?
    else {
        return Ok(None);
    };
    let daemon_architecture = super::provider::daemon_architecture(&before)
        .ok_or(HostError::Identity)?
        .to_owned();
    // Release preparation may take minutes. Start the fresh-sample budget only
    // after the verified image has been rebound to the selected daemon.
    let deadline = Deadline::new();
    let selected = info(docker, &deadline).await?;
    if !same_guest(&selected, &engine_id, &root, &daemon_architecture)? {
        return Err(HostError::Identity);
    }
    let (cpu_count, memory_total) = guest_capacity(&selected)?;
    let image_evidence = candidate.startup_evidence().ok_or(HostError::Identity)?;
    let image = verified_image(candidate)?;
    let projection =
        projection(journal, &root, &engine_id, image, &deadline, doctor_ticket).await?;
    let output = execute::run(
        docker,
        journal,
        &projection,
        image_evidence,
        memory_total,
        &deadline,
    )
    .await?;
    let after = info(docker, &deadline).await?;
    if !same_guest(&after, &engine_id, &root, &daemon_architecture)? {
        return Err(HostError::Identity);
    }
    if guest_capacity(&after)? != (cpu_count, memory_total) {
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
        output.container_host_pid,
        memory_total,
        engine_id,
        root_digest,
        output.sample_generation_id,
        output.observed_at,
        output.startup_evidence?,
    )
}

fn verified_image(candidate: VerifiedCandidate) -> Result<VerifiedProbeImage, HostError> {
    VerifiedProbeImage::from_verified_provider(
        candidate.runtime_id,
        candidate.platform,
        candidate.daemon_architecture,
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
    let (cpu_count, memory_total) = guest_capacity(info)?;
    if engine_id != bound {
        return Err(HostError::Identity);
    }
    Ok((engine_id, root, cpu_count, memory_total))
}

fn guest_capacity(info: &SystemInfo) -> Result<(u32, u64), HostError> {
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

async fn projection(
    journal: &Journal,
    root: &DockerRoot,
    engine_id: &str,
    image: VerifiedProbeImage,
    deadline: &Deadline,
    doctor_ticket: Option<&DoctorProbeTicket>,
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
        purpose: ResourceProbePurpose::Capacity,
        request_sha256: projection.request_sha256.clone(),
    };
    match doctor_ticket {
        Some(ticket) => match deadline
            .within(journal.prepare_probe_for_doctor_request(ticket, seed))
            .await??
        {
            DoctorProbePrepareResult::Prepared(_) | DoctorProbePrepareResult::AlreadyLinked(_) => {}
            DoctorProbePrepareResult::WaitingForProbe => return Err(HostError::Journal),
        },
        None => deadline.within(journal.prepare_probe(seed)).await??,
    }
    Ok(projection)
}

fn same_guest(
    info: &SystemInfo,
    engine_id: &str,
    root: &DockerRoot,
    daemon_architecture: &str,
) -> Result<bool, HostError> {
    let current_id = info.id.as_deref().ok_or(HostError::Identity)?;
    let current_root = DockerRoot::parse(info.docker_root_dir.as_deref().ok_or(HostError::Path)?)?;
    Ok(super::provider::daemon_platform_matches(info)
        && super::provider::daemon_architecture(info) == Some(daemon_architecture)
        && current_id == engine_id
        && current_root.digest() == root.digest())
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

    pub(super) fn until(deadline: Instant) -> Self {
        let now = Instant::now();
        Self {
            began: now,
            timeout: deadline.saturating_duration_since(now),
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
