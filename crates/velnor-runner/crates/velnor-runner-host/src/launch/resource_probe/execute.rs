//! One current-process probe attempt; samples are returned only after cleanup.

use std::time::{Duration, Instant};

use bollard::Docker;

use crate::error::HostError;
use crate::journal::{Journal, ProbePhase};

use super::inspect;
use super::lifecycle::Deadline;
use super::ops;
use super::projection::ProbeProjection;
use super::provider::ProbeImagePreparationEvidence;
use super::record::ProbeRecord;
use super::recover;

#[path = "execute/owned_cgroups.rs"]
mod owned_cgroups;
pub(super) use owned_cgroups::run_owned_cgroups;

/// Evidence for one probe image and its cleanup on the selected engine.
pub(in crate::launch) struct ProbeCleanupEvidence {
    operation_id: String,
    engine_id: String,
    docker_root_digest: String,
    operation_name: String,
    container_id: String,
}

impl ProbeCleanupEvidence {
    pub(in crate::launch) fn after_journal_retirement(
        proof: &super::VerifiedProbeCleanup,
    ) -> Result<Self, HostError> {
        let container_id = proof.container_id().ok_or(HostError::Ownership)?;
        if proof.phase() != crate::journal::ProbePhase::RemoveRequested {
            return Err(HostError::Ownership);
        }
        Ok(Self {
            operation_id: proof.operation_id().to_owned(),
            engine_id: proof.engine_id().to_owned(),
            docker_root_digest: proof.docker_root_digest().to_owned(),
            operation_name: proof.operation_name().to_owned(),
            container_id: container_id.to_owned(),
        })
    }

    pub(in crate::launch) fn operation_id(&self) -> &str {
        &self.operation_id
    }
    pub(in crate::launch) fn engine_id(&self) -> &str {
        &self.engine_id
    }
    pub(in crate::launch) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }
    pub(in crate::launch) fn operation_name(&self) -> &str {
        &self.operation_name
    }
    pub(in crate::launch) fn container_id(&self) -> &str {
        &self.container_id
    }
}

/// Evidence for the probe and image after cleanup completes.
pub(in crate::launch) struct ProbeStartupEvidence {
    cleanup: ProbeCleanupEvidence,
    image: ProbeImagePreparationEvidence,
}

impl ProbeStartupEvidence {
    pub(in crate::launch) fn new(
        cleanup: ProbeCleanupEvidence,
        image: ProbeImagePreparationEvidence,
    ) -> Self {
        Self { cleanup, image }
    }

    pub(in crate::launch) fn cleanup(&self) -> &ProbeCleanupEvidence {
        &self.cleanup
    }
    pub(in crate::launch) fn image(&self) -> &ProbeImagePreparationEvidence {
        &self.image
    }

    #[cfg(test)]
    pub(in crate::launch) fn test_fixture(
        generation: &str,
        engine_id: &str,
        docker_root_digest: &str,
    ) -> Self {
        Self {
            cleanup: ProbeCleanupEvidence {
                operation_id: generation.to_owned(),
                engine_id: engine_id.to_owned(),
                docker_root_digest: docker_root_digest.to_owned(),
                operation_name: format!("velnor-resource-probe-{generation}"),
                container_id: "d".repeat(64),
            },
            image: ProbeImagePreparationEvidence::test_fixture(
                engine_id,
                docker_root_digest,
                "aarch64",
                &"a".repeat(40),
            ),
        }
    }
}

pub(super) struct ObservedProbeRecord {
    pub(super) record: ProbeRecord,
    pub(super) container_host_pid: u32,
    pub(super) sample_generation_id: String,
    pub(super) observed_at: Instant,
    pub(super) startup_evidence: Option<ProbeStartupEvidence>,
}

pub(super) async fn run(
    docker: &Docker,
    journal: &Journal,
    projection: &ProbeProjection,
    image: ProbeImagePreparationEvidence,
    memory_total: u64,
    deadline: &Deadline,
) -> Result<ObservedProbeRecord, HostError> {
    let result = execute(docker, journal, projection, memory_total, deadline, image).await;
    if result.is_err() {
        recover_current(docker, journal, projection, deadline).await;
    }
    result
}

async fn execute(
    docker: &Docker,
    journal: &Journal,
    projection: &ProbeProjection,
    memory_total: u64,
    deadline: &Deadline,
    image: ProbeImagePreparationEvidence,
) -> Result<ObservedProbeRecord, HostError> {
    deadline
        .within(journal.transition_probe(
            &projection.operation_id,
            ProbePhase::Prepared,
            ProbePhase::CreateRequested,
        ))
        .await??;
    recover::verify_current_projection(docker, journal, projection, deadline).await?;
    let id = ops::create(docker, projection, deadline).await?;
    deadline
        .within(journal.bind_probe_container(&projection.operation_id, &id))
        .await??;
    recover::verify_current_projection(docker, journal, projection, deadline).await?;
    verify_created(docker, projection, &id, deadline).await?;
    let mut record =
        execute_started(docker, journal, projection, &id, memory_total, deadline).await?;
    let row = deadline
        .within(journal.active_probe())
        .await??
        .ok_or(HostError::Journal)?;
    let cleanup = recover::cleanup(docker, journal, &row, projection, deadline).await?;
    record.startup_evidence = Some(ProbeStartupEvidence::new(cleanup, image));
    Ok(record)
}

async fn verify_created(
    docker: &Docker,
    projection: &ProbeProjection,
    id: &str,
    deadline: &Deadline,
) -> Result<(), HostError> {
    let by_id = inspect::by_reference(docker, id, deadline)
        .await?
        .ok_or(HostError::Ownership)?;
    let by_name = inspect::by_reference(docker, &projection.name, deadline)
        .await?
        .ok_or(HostError::Ownership)?;
    if inspect::matches(projection, &by_id, id)
        && inspect::matches(projection, &by_name, id)
        && inspect::created(&by_id)
    {
        Ok(())
    } else {
        Err(HostError::Ownership)
    }
}

async fn execute_started(
    docker: &Docker,
    journal: &Journal,
    projection: &ProbeProjection,
    id: &str,
    memory_total: u64,
    deadline: &Deadline,
) -> Result<ObservedProbeRecord, HostError> {
    deadline
        .within(journal.transition_probe(
            &projection.operation_id,
            ProbePhase::ContainerCreated,
            ProbePhase::StartRequested,
        ))
        .await??;
    recover::verify_current_projection(docker, journal, projection, deadline).await?;
    let start_result = ops::start(docker, id, deadline).await;
    recover::verify_current_projection(docker, journal, projection, deadline).await?;
    let container_host_pid = if start_result.is_err() {
        verify_started_after_lost_reply(docker, projection, id, deadline).await?
    } else {
        verify_started(docker, projection, id, deadline).await?
    };
    deadline
        .within(journal.transition_probe(
            &projection.operation_id,
            ProbePhase::StartRequested,
            ProbePhase::Started,
        ))
        .await??;
    deadline
        .within(journal.transition_probe(
            &projection.operation_id,
            ProbePhase::Started,
            ProbePhase::WaitRequested,
        ))
        .await??;
    ops::wait_success(docker, id, deadline).await?;
    verify_exit(docker, projection, id, deadline).await?;
    deadline
        .within(journal.transition_probe(
            &projection.operation_id,
            ProbePhase::WaitRequested,
            ProbePhase::Waited,
        ))
        .await??;
    deadline
        .within(journal.transition_probe(
            &projection.operation_id,
            ProbePhase::Waited,
            ProbePhase::LogsRequested,
        ))
        .await??;
    let output = ops::stdout(docker, id, deadline).await?;
    let record = ProbeRecord::parse(&output, memory_total).ok_or(HostError::Frame)?;
    if record.probe_host_pid != container_host_pid
        || record.probe_namespace_pid != container_host_pid
    {
        return Err(HostError::Identity);
    }
    let observed_at = Instant::now();
    deadline
        .within(journal.transition_probe(
            &projection.operation_id,
            ProbePhase::LogsRequested,
            ProbePhase::OutputObserved,
        ))
        .await??;
    Ok(ObservedProbeRecord {
        record,
        container_host_pid,
        sample_generation_id: projection.operation_id.clone(),
        observed_at,
        startup_evidence: None,
    })
}

async fn verify_started_after_lost_reply(
    docker: &Docker,
    projection: &ProbeProjection,
    id: &str,
    deadline: &Deadline,
) -> Result<u32, HostError> {
    verify_started_pid(docker, projection, id, deadline)
        .await
        .map_err(|_| HostError::ContainerStartUncertain)
}

async fn verify_started(
    docker: &Docker,
    projection: &ProbeProjection,
    id: &str,
    deadline: &Deadline,
) -> Result<u32, HostError> {
    verify_started_pid(docker, projection, id, deadline).await
}

async fn verify_started_pid(
    docker: &Docker,
    projection: &ProbeProjection,
    id: &str,
    deadline: &Deadline,
) -> Result<u32, HostError> {
    let inspection_started = Instant::now();
    loop {
        let response = inspect::by_reference(docker, id, deadline)
            .await?
            .ok_or(HostError::ContainerStartUncertain)?;
        if !inspect::matches(projection, &response, id) {
            return Err(HostError::Ownership);
        }
        if let Some(pid) = inspect::running_host_pid(&response) {
            return Ok(pid);
        }
        if inspect::exited(&response) || inspection_started.elapsed() >= Duration::from_secs(2) {
            return Err(HostError::ContainerStartUncertain);
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn verify_exit(
    docker: &Docker,
    projection: &ProbeProjection,
    id: &str,
    deadline: &Deadline,
) -> Result<(), HostError> {
    let response = inspect::by_reference(docker, id, deadline)
        .await?
        .ok_or(HostError::Ownership)?;
    if inspect::matches(projection, &response, id) && inspect::exited_successfully(&response) {
        Ok(())
    } else {
        Err(HostError::Docker)
    }
}

async fn recover_current(
    docker: &Docker,
    journal: &Journal,
    projection: &ProbeProjection,
    deadline: &Deadline,
) {
    let Ok(Ok(Some(row))) = deadline.within(journal.active_probe()).await else {
        return;
    };
    if row.phase == ProbePhase::Prepared {
        let _aborted = deadline
            .within(journal.abort_prepared_probe(&row.operation_id))
            .await;
        return;
    }
    if let Err(error) = recover::cleanup(docker, journal, &row, projection, deadline).await {
        if matches!(
            error,
            HostError::DockerTimeout | HostError::Identity | HostError::Path
        ) {
            return;
        }
        let _quarantined = deadline
            .within(journal.quarantine_probe(&row.operation_id))
            .await;
    }
}
