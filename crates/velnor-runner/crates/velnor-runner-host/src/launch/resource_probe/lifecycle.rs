//! One bounded probe attempt, including prior-operation recovery.

use bollard::{Docker, models::SystemInfo};
use uuid::Uuid;

use crate::error::HostError;
use crate::journal::{
    DoctorProbeTicket, Journal, ProbePhase, ProbeRow, ProbeSeed, ResourceProbePurpose,
};

use super::projection::{DockerRoot, ProbeProjection};
use super::provider::{ImageProvider, VerifiedCandidate};
use super::sample::Observation;

#[path = "lifecycle/support.rs"]
mod support;
use super::{execute, recover};
pub(super) use support::{Deadline, bind_info, guest_capacity, info, observation, same_guest};
use support::{projection, verified_image};

const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

#[path = "lifecycle/publication_image.rs"]
mod publication_image;
pub(super) use publication_image::verify_publication_image_until;

pub(super) async fn collect<P: ImageProvider>(
    docker: &Docker,
    journal: &Journal,
    provider: &P,
) -> Result<Option<Observation>, HostError> {
    let _operation = super::readiness::lock_operation(journal).await?;
    collect_for_request(docker, journal, provider, None).await
}

pub(super) async fn collect_for_doctor<P: ImageProvider>(
    docker: &Docker,
    journal: &Journal,
    provider: &P,
    ticket: &DoctorProbeTicket,
) -> Result<Option<Observation>, HostError> {
    let _operation = super::readiness::lock_operation(journal).await?;
    collect_for_request(docker, journal, provider, Some(ticket)).await
}

pub(super) async fn collect_owned_cgroups<P: ImageProvider>(
    docker: &Docker,
    journal: &Journal,
    provider: &P,
    subjects: &super::owned_cgroups::OwnedCgroupSubjects,
) -> Result<Option<super::owned_cgroups::VerifiedOwnedCgroups>, HostError> {
    let _operation = super::readiness::lock_operation(journal).await?;
    let initial = Deadline::new();
    let active = initial.within(journal.active_probe()).await??;
    let before = info(docker, &initial).await?;
    let (engine_id, root, _, _) = bind_info(&before, journal, &initial).await?;
    if let Some(row) = active {
        Box::pin(recover::active(
            docker, journal, &before, &root, &initial, row,
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
    let _operation = super::readiness::lock_operation(journal).await?;
    let row = journal.begin_doctor_probe_recovery(ticket).await?;
    let deadline = Deadline::new();
    let current = info(docker, &deadline).await?;
    let (engine_id, root, _, _) = bind_info(&current, journal, &deadline).await?;
    if engine_id != row.engine_id || root.digest() != row.docker_root_digest {
        return Err(HostError::Identity);
    }
    Box::pin(recover::active(
        docker,
        journal,
        &current,
        &root,
        &deadline,
        row.clone(),
    ))
    .await?;
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
    collect_for_request_with_recovery(docker, journal, provider, doctor_ticket, true, None).await
}

pub(super) async fn collect_for_readiness<P: ImageProvider>(
    docker: &Docker,
    journal: &Journal,
    provider: &P,
) -> Result<Option<Observation>, HostError> {
    collect_for_request_with_recovery(docker, journal, provider, None, false, None).await
}

#[cfg(test)]
pub(super) async fn collect_for_readiness_with_operation_id<P: ImageProvider>(
    docker: &Docker,
    journal: &Journal,
    provider: &P,
    operation_id: &str,
) -> Result<Option<Observation>, HostError> {
    collect_for_request_with_recovery(docker, journal, provider, None, false, Some(operation_id))
        .await
}

async fn collect_for_request_with_recovery<P: ImageProvider>(
    docker: &Docker,
    journal: &Journal,
    provider: &P,
    doctor_ticket: Option<&DoctorProbeTicket>,
    recover_existing: bool,
    operation_id: Option<&str>,
) -> Result<Option<Observation>, HostError> {
    let preparation_deadline = Deadline::new();
    let mut active = preparation_deadline
        .within(journal.active_probe())
        .await??;
    if active.is_some() && !recover_existing {
        return Ok(None);
    }
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
    recover_active_probe(
        docker,
        journal,
        &before,
        &root,
        &preparation_deadline,
        active,
    )
    .await?;
    let Some((candidate, daemon_architecture)) =
        candidate_for_current_daemon(docker, journal, provider, &engine_id, &before).await?
    else {
        return Ok(None);
    };
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
    let projection = projection(
        journal,
        &root,
        &engine_id,
        image,
        &deadline,
        doctor_ticket,
        operation_id,
    )
    .await?;
    let output = Box::pin(execute::run(
        docker,
        journal,
        &projection,
        image_evidence,
        memory_total,
        &deadline,
    ))
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

async fn recover_active_probe(
    docker: &Docker,
    journal: &Journal,
    before: &SystemInfo,
    root: &DockerRoot,
    deadline: &Deadline,
    active: Option<ProbeRow>,
) -> Result<(), HostError> {
    if let Some(row) = active {
        Box::pin(recover::active(
            docker, journal, before, root, deadline, row,
        ))
        .await?;
    }
    Ok(())
}

async fn candidate_for_current_daemon<P: ImageProvider>(
    docker: &Docker,
    journal: &Journal,
    provider: &P,
    engine_id: &str,
    before: &SystemInfo,
) -> Result<Option<(VerifiedCandidate, String)>, HostError> {
    let Some(candidate) = provider
        .verified_candidate(docker, journal, engine_id, before)
        .await?
    else {
        return Ok(None);
    };
    let daemon_architecture = super::provider::daemon_architecture(before)
        .ok_or(HostError::Identity)?
        .to_owned();
    Ok(Some((candidate, daemon_architecture)))
}
