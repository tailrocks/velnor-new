use super::admission::JOB_LIMIT;
use super::helpers::{
    ProvisionedRunner, Scratch, binding, created_scope, prepare_initialized_listener_delivery,
    provision_acquired_request_with_initialization, runner_name,
};
use crate::error::HostError;
use crate::journal::{ConnectionJournal, Journal, ReservationState};
use crate::launch::bootstrap::test_listener_delivery_receipt;
use crate::reconcile::LaunchPhase;
use velnor_runner_github::RunnerReference;

struct StartingRunner {
    _scratch: Scratch,
    scope: ConnectionJournal,
    provisioned: ProvisionedRunner,
    runner: RunnerReference,
    delivery: crate::journal::ListenerDeliveryClaim,
}

async fn starting_runner(nonce: char) -> Result<StartingRunner, HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal.configure_capacity(JOB_LIMIT).await?;
    let binding = binding("essential", 101)?;
    let scope = created_scope(&journal, &binding, 501).await?;
    let initialized =
        provision_acquired_request_with_initialization(&scope, 70, nonce, nonce).await?;
    let provisioned = initialized.provisioned.clone();
    let runner = RunnerReference {
        id: 770,
        name: runner_name(nonce),
        runner_scale_set_id: 501,
    };
    let delivery =
        prepare_initialized_listener_delivery(&scope, &binding, &initialized, &runner, 70).await?;
    Ok(StartingRunner {
        _scratch: scratch,
        scope,
        provisioned,
        runner,
        delivery,
    })
}

#[tokio::test]
async fn listener_delivery_finalizes_exact_prebound_container_ids() -> Result<(), HostError> {
    let start = starting_runner('8').await?;
    let proof = test_listener_delivery_receipt(
        start.provisioned.reservation_id,
        start.provisioned.intent_id,
        super::helpers::TEST_ENGINE_ID.to_owned(),
        start.provisioned.docker_root_digest.clone(),
        start.provisioned.runner_container_id.clone(),
        start.runner.clone(),
    )?;
    start
        .scope
        .record_listener_delivered(&start.delivery, &proof)
        .await?;

    let intent = start.scope.intent(start.provisioned.intent_id).await?;
    assert_eq!(
        intent.docker_id.as_deref(),
        Some(start.provisioned.runner_container_id.as_str())
    );
    assert_eq!(
        intent.dind_id.as_deref(),
        Some(start.provisioned.dind_container_id.as_str())
    );
    assert_eq!(intent.launch_phase, Some(LaunchPhase::WorkerReady));
    let reservation = start
        .scope
        .reservation(&start.provisioned.assignment)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(reservation.state, ReservationState::Reserved);
    Ok(())
}

#[tokio::test]
async fn one_source_backed_job_facts_publication_survives_reopen() -> Result<(), HostError> {
    let start = starting_runner('b').await?;
    let StartingRunner {
        _scratch,
        scope,
        provisioned,
        runner: _,
        delivery: _,
    } = start;
    let path = _scratch.file();
    let reservation_id = provisioned.reservation_id;
    drop(scope);
    drop(provisioned);

    let journal = Journal::open(&path).await?;
    let connection = journal.connection().await?;
    let mut accounting = connection
        .query(
            "SELECT (SELECT COUNT(*) FROM capacity_reservation_provisioning_roles WHERE reservation_id = ?1), (SELECT COUNT(*) FROM capacity_reservation_provisioning_roles WHERE reservation_id = ?1 AND role = 'engine-metadata'), (SELECT COUNT(*) FROM capacity_reservation_provisioning_objects WHERE reservation_id = ?1), (SELECT COUNT(*) FROM capacity_reservation_provisioning_objects WHERE reservation_id = ?1 AND role = 'engine-metadata'), (SELECT COUNT(*) FROM capacity_reservation_provisioning_pools WHERE reservation_id = ?1)",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let accounting = accounting
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    assert_eq!(
        accounting.get::<i64>(0).map_err(|_| HostError::Journal)?,
        11
    );
    assert_eq!(accounting.get::<i64>(1).map_err(|_| HostError::Journal)?, 1);
    assert_eq!(accounting.get::<i64>(2).map_err(|_| HostError::Journal)?, 5);
    assert_eq!(accounting.get::<i64>(3).map_err(|_| HostError::Journal)?, 1);
    assert_eq!(accounting.get::<i64>(4).map_err(|_| HostError::Journal)?, 9);
    let mut rows = connection
        .query(
            "SELECT COUNT(*), MIN(target), MAX(target) FROM capacity_reservation_provisioning_publications WHERE reservation_id = ?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    assert_eq!(row.get::<i64>(0).map_err(|_| HostError::Journal)?, 1);
    assert_eq!(
        row.get::<String>(1).map_err(|_| HostError::Journal)?,
        "/opt/velnor/job-facts/storage-cache-admission.json"
    );
    assert_eq!(
        row.get::<String>(2).map_err(|_| HostError::Journal)?,
        "/opt/velnor/job-facts/storage-cache-admission.json"
    );
    Ok(())
}

#[tokio::test]
async fn mismatched_delivery_proof_keeps_the_prebound_ids_and_charge() -> Result<(), HostError> {
    let start = starting_runner('a').await?;
    let proof = test_listener_delivery_receipt(
        start.provisioned.reservation_id,
        start.provisioned.intent_id,
        super::helpers::TEST_ENGINE_ID.to_owned(),
        start.provisioned.docker_root_digest.clone(),
        "c".repeat(64),
        start.runner.clone(),
    )?;
    assert!(
        start
            .scope
            .record_listener_delivered(&start.delivery, &proof)
            .await
            .is_err()
    );

    let intent = start.scope.intent(start.provisioned.intent_id).await?;
    assert_eq!(
        intent.docker_id.as_deref(),
        Some(start.provisioned.runner_container_id.as_str())
    );
    assert_eq!(
        intent.dind_id.as_deref(),
        Some(start.provisioned.dind_container_id.as_str())
    );
    assert_eq!(intent.launch_phase, Some(LaunchPhase::JobFactsPublished));
    let reservation = start
        .scope
        .reservation(&start.provisioned.assignment)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(reservation.state, ReservationState::Reserved);
    Ok(())
}
