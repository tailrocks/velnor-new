use bollard::Docker;

use crate::error::HostError;
use crate::journal::{Journal, ProbePhase, ResourceProbePurpose};

use super::super::lifecycle::Deadline;
use super::super::ops;
use super::super::owned_cgroups::{OwnedCgroupSubjects, parse_record};
use super::super::projection::ProbeProjection;
use super::super::recover;
use super::{ProbeCleanupEvidence, recover_current, verify_created};

pub(in crate::launch::resource_probe) async fn run_owned_cgroups(
    docker: &Docker,
    journal: &Journal,
    projection: &ProbeProjection,
    subjects: &OwnedCgroupSubjects,
    deadline: &Deadline,
) -> Result<
    (
        velnor_resource_probe::worker_binding::OwnedCgroupObservation,
        ProbeCleanupEvidence,
        u32,
    ),
    HostError,
> {
    let result = execute_owned_cgroups(docker, journal, projection, subjects, deadline).await;
    if result.is_err() {
        recover_current(docker, journal, projection, deadline).await;
    }
    result
}

async fn execute_owned_cgroups(
    docker: &Docker,
    journal: &Journal,
    projection: &ProbeProjection,
    subjects: &OwnedCgroupSubjects,
    deadline: &Deadline,
) -> Result<
    (
        velnor_resource_probe::worker_binding::OwnedCgroupObservation,
        ProbeCleanupEvidence,
        u32,
    ),
    HostError,
> {
    if projection.purpose != ResourceProbePurpose::OwnedCgroups {
        return Err(HostError::Identity);
    }
    let request_line = subjects.request_line(&projection.operation_id, projection.root.path())?;
    let request = subjects.request(
        projection.operation_id.clone(),
        projection.root.path().to_owned(),
    );
    if subjects.request_sha256(&projection.operation_id, projection.root.path())?
        != projection.request_sha256
    {
        return Err(HostError::Identity);
    }
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
    deadline
        .within(journal.transition_probe(
            &projection.operation_id,
            ProbePhase::ContainerCreated,
            ProbePhase::StartRequested,
        ))
        .await??;
    recover::verify_current_projection(docker, journal, projection, deadline).await?;
    let attachment = ops::owned_cgroups(docker, projection, &id, deadline).await?;
    let host_pid = attachment.host_pid();
    recover::verify_current_projection(docker, journal, projection, deadline).await?;
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
    let output =
        ops::owned_cgroups_output(docker, projection, &id, attachment, &request_line, deadline)
            .await?;
    let observation = parse_record(&output, &request)?;
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
    deadline
        .within(journal.transition_probe(
            &projection.operation_id,
            ProbePhase::LogsRequested,
            ProbePhase::OutputObserved,
        ))
        .await??;
    let row = deadline
        .within(journal.active_probe())
        .await??
        .ok_or(HostError::Journal)?;
    let cleanup = recover::cleanup(docker, journal, &row, projection, deadline).await?;
    Ok((observation, cleanup, host_pid))
}
