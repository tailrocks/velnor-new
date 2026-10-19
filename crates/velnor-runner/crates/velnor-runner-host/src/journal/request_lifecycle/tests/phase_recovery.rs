use super::admission::JOB_LIMIT;
use super::helpers::{
    Scratch, binding, created_scope, event_source, provision_physical_intent, record_acquired,
    runner_name,
};
use crate::error::HostError;
use crate::journal::{CapacityLimits, Journal, ReservationState};
use crate::reconcile::LaunchPhase;
use velnor_runner_github::RunnerReference;

#[tokio::test]
async fn runner_started_phase_and_capacity_survive_restart() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: JOB_LIMIT.jobs,
            cpu_millicores: JOB_LIMIT.cpu_millicores,
            memory_bytes: JOB_LIMIT.memory_bytes,
        })
        .await?;
    let binding = binding("essential", 101)?;
    let scope = created_scope(&journal, &binding, 501).await?;
    record_acquired(&scope, 70).await?;
    let provisioned = provision_physical_intent(&scope, '8').await?;
    let physical = provisioned.assignment.clone();
    let intent_id = provisioned.intent_id;
    let runner = RunnerReference {
        id: 770,
        name: runner_name('8'),
        runner_scale_set_id: 501,
    };
    super::helpers::finish_listener_delivery(&scope, &provisioned, &runner).await?;
    let request = scope
        .assignment_identity(crate::journal::AssignmentKey::AssignedRequest { request_id: 70 })
        .await?;
    assert_eq!(
        scope
            .record_job_started(&request, 701, &event_source(101, 1070), &runner, None)
            .await?,
        intent_id
    );
    assert_eq!(
        scope.intent(intent_id).await?.launch_phase,
        Some(LaunchPhase::RunnerStarted)
    );
    drop(scope);
    drop(journal);

    let journal = Journal::open(&path).await?;
    let scope = journal.connection_scope(&binding).await?;
    assert_eq!(
        scope.intent(intent_id).await?.launch_phase,
        Some(LaunchPhase::RunnerStarted)
    );
    assert_eq!(
        scope.reservation(&physical).await?.map(|row| row.state),
        Some(ReservationState::Reserved)
    );
    Ok(())
}
