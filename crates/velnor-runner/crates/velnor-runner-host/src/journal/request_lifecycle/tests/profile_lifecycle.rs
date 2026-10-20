use super::helpers::{
    TEST_ENGINE_ID, record_owned_network_cleanup, record_owned_storage_cleanup,
    record_runner_absent_for_cleanup, record_runner_completion_for_cleanup,
    settle_storage_metadata,
};
use super::review_counterexamples::ready;
use crate::connect::SetOwnership;
use crate::error::HostError;
use crate::journal::{
    AssignmentKey, Journal, MetadataSettlementResult, OperationalCapacityLimits, ProbeSeed,
    ReservationRequest, ReservationResult, ReservationState, StorageProfile, fixture_admission,
    fixture_profile,
};
use crate::launch::StartPermit;

fn next_profile(current: &StorageProfile) -> Result<StorageProfile, HostError> {
    StorageProfile::new(
        "f".repeat(32),
        current.pools().to_vec(),
        current.roles().to_vec(),
    )
}

#[tokio::test]
async fn profile_generation_change_waits_for_hold_settlement_across_reopen() -> Result<(), HostError>
{
    let fixture = ready().await?;
    let previous = assert_profile_hold(&fixture).await?;
    let next = next_profile(&previous)?;

    let path = fixture.scratch.file();
    let binding = fixture.binding.clone();
    drop(fixture.scope);
    drop(fixture.journal);

    let journal = Journal::open(&path).await?;
    let scope = journal.connection_scope(&binding).await?;
    assert_eq!(
        scope
            .reservation(&fixture.physical)
            .await?
            .map(|row| row.state),
        Some(ReservationState::Reserved)
    );
    assert_eq!(
        settle_storage_metadata(&scope, &fixture.provisioned, 'd').await?,
        MetadataSettlementResult::Settled
    );
    assert_eq!(
        scope
            .reservation(&fixture.physical)
            .await?
            .map(|row| row.state),
        Some(ReservationState::Released)
    );
    drop(scope);
    drop(journal);

    let journal = Journal::open(&path).await?;
    journal.configure_storage_profile(next.clone()).await?;
    assert_eq!(
        journal
            .current_storage_profile()
            .await?
            .ok_or(HostError::Journal)?
            .generation_id(),
        next.generation_id()
    );
    Ok(())
}

async fn assert_profile_hold(
    fixture: &super::review_counterexamples::Ready,
) -> Result<StorageProfile, HostError> {
    let reservation = fixture
        .scope
        .reservation(&fixture.physical)
        .await?
        .ok_or(HostError::Journal)?;
    let cleanup_generation = record_runner_completion_for_cleanup(
        &fixture.scope,
        &fixture.request,
        fixture.intent,
        Some(701),
        703,
        &super::helpers::event_source(101, 1070),
        &fixture.runner,
    )
    .await?;
    record_owned_storage_cleanup(&fixture.scope, &fixture.provisioned).await?;
    record_owned_network_cleanup(&fixture.scope, reservation.reservation_id).await?;
    fixture
        .scope
        .record_remote_cleanup(reservation.reservation_id)
        .await?;
    fixture
        .scope
        .record_local_cleanup(reservation.reservation_id)
        .await?;
    record_runner_absent_for_cleanup(&fixture.scope, fixture.intent, cleanup_generation).await?;

    let previous = fixture
        .scope
        .current_storage_profile()
        .await?
        .ok_or(HostError::Journal)?;
    let next = next_profile(&previous)?;
    fixture
        .journal
        .configure_storage_profile(previous.clone())
        .await?;
    assert!(
        fixture
            .journal
            .configure_storage_profile(next)
            .await
            .is_err(),
        "An active capacity reservation must keep its storage profile current."
    );
    assert_eq!(
        fixture
            .scope
            .current_storage_profile()
            .await?
            .ok_or(HostError::Journal)?
            .generation_id(),
        previous.generation_id()
    );
    Ok(previous)
}

#[tokio::test]
async fn profile_switch_serializes_with_settlement_claim() -> Result<(), HostError> {
    let fixture = ready().await?;
    let reservation = fixture
        .scope
        .reservation(&fixture.physical)
        .await?
        .ok_or(HostError::Journal)?;
    record_owned_storage_cleanup(&fixture.scope, &fixture.provisioned).await?;
    record_owned_network_cleanup(&fixture.scope, reservation.reservation_id).await?;
    fixture
        .scope
        .record_remote_cleanup(reservation.reservation_id)
        .await?;
    fixture
        .scope
        .record_local_cleanup(reservation.reservation_id)
        .await?;
    fixture
        .journal
        .configure_capacity_with_operational_limits(
            super::admission::JOB_LIMIT,
            OperationalCapacityLimits {
                cpu_millicores: 1_000,
                memory_bytes: 128 * 1024 * 1024,
            },
        )
        .await?;
    let previous = fixture
        .scope
        .current_storage_profile()
        .await?
        .ok_or(HostError::Journal)?;
    let next = next_profile(&previous)?;
    let operation_id = "9".repeat(32);
    let seed = ProbeSeed {
        operation_id: operation_id.clone(),
        instance_id: fixture.journal.instance_id().await?,
        engine_id: TEST_ENGINE_ID.to_owned(),
        docker_root_digest: fixture.provisioned.docker_root_digest.clone(),
        source_revision: "b".repeat(40),
        runtime_image_id: format!("sha256:{}", "c".repeat(64)),
        image_binding_digest: "d".repeat(64),
        operation_name: format!("velnor-resource-probe-{operation_id}"),
        projection_digest: "e".repeat(64),
        purpose: crate::journal::ResourceProbePurpose::Capacity,
        request_sha256: crate::journal::capacity_request_sha256(),
    };

    let (profile_result, claim_result) = tokio::join!(
        fixture.journal.configure_storage_profile(next),
        fixture
            .scope
            .prepare_storage_metadata_settlement_probe(reservation.reservation_id, seed),
    );
    assert!(profile_result.is_err());
    let claim = claim_result?;
    assert_eq!(claim.profile_generation_id(), previous.generation_id());
    assert_eq!(
        fixture
            .scope
            .current_storage_profile()
            .await?
            .ok_or(HostError::Journal)?
            .generation_id(),
        previous.generation_id()
    );
    Ok(())
}

#[tokio::test]
async fn profile_switch_holds_every_charged_reservation_without_provisioning()
-> Result<(), HostError> {
    for state in ["reserved", "uncertain", "recovering"] {
        assert_charged_unprovisioned_state_blocks_profile_switch(state).await?;
    }
    Ok(())
}

mod legacy_cases;

async fn assert_charged_unprovisioned_state_blocks_profile_switch(
    requested_state: &str,
) -> Result<(), HostError> {
    let scratch = super::helpers::Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    journal
        .configure_capacity(super::admission::JOB_LIMIT)
        .await?;
    let binding = super::helpers::binding("profile-legacy", 101)?;
    let scope = super::helpers::created_scope(&journal, &binding, 501).await?;
    let old = fixture_profile();
    journal.configure_storage_profile(old.clone()).await?;

    let reservation_id = reserve_state_for_profile_test(&scope, requested_state).await?;

    let connection = journal.connection().await?;
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM capacity_reservation_provisioning WHERE reservation_id = ?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let count = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    assert_eq!(count, 0, "fixture must lack a provisioning record");
    drop(connection);

    journal.configure_storage_profile(old.clone()).await?;
    let next = next_profile(&old)?;
    drop(scope);
    drop(journal);

    let journal = Journal::open(&path).await?;
    assert!(
        journal.configure_storage_profile(next).await.is_err(),
        "{requested_state} reservation without provisioning still holds capacity"
    );
    assert_eq!(
        journal
            .current_storage_profile()
            .await?
            .ok_or(HostError::Journal)?
            .generation_id(),
        old.generation_id()
    );
    Ok(())
}

async fn reserve_state_for_profile_test(
    scope: &crate::journal::ConnectionJournal,
    requested_state: &str,
) -> Result<i64, HostError> {
    let assignment = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 77 })
        .await?;
    let generation = "9".repeat(32);
    let permit = StartPermit::test_fixture_with_engine_id(&generation, TEST_ENGINE_ID);
    let root = permit.docker_root_digest().to_owned();
    let storage = fixture_admission(&permit)?;
    let result = scope
        .try_reserve_with_storage(
            ReservationRequest {
                assignment: &assignment,
                engine_id: TEST_ENGINE_ID,
                docker_root_digest: &root,
                cpu_millicores: 2_000,
                memory_bytes: 8_u64 << 30,
                sample_generation_id: &generation,
            },
            permit,
            storage,
        )
        .await?;
    let ReservationResult::Reserved { reservation_id } = result else {
        return Err(HostError::Journal);
    };
    match requested_state {
        "reserved" => {}
        "uncertain" => scope.mark_uncertain(reservation_id).await?,
        "recovering" => scope.mark_recovering(reservation_id).await?,
        _ => return Err(HostError::Journal),
    }
    Ok(reservation_id)
}
