use std::collections::BTreeMap;

use crate::error::HostError;
use crate::journal::{
    AssignedAcquireResult, AssignmentKey, CapacityLimits, Journal, RequestAcquireState,
    RequestPhysicalReservationResult, ReservationRequest, SourceAuthorizationResult,
    fixture_storage_provisioning_admission,
};
use crate::launch::StartPermit;

use super::helpers::{Scratch, TEST_ENGINE_ID, binding, created_scope, receipt};

const SET_ID: i64 = 501;
const REQUEST_ID: i64 = 811;
const JOB_MEMORY: u64 = 8_589_934_592;

#[tokio::test]
async fn pending_network_state_keeps_the_no_acquire_charge() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 2_000,
            memory_bytes: 25_769_803_776,
        })
        .await?;
    let binding = binding("acquire-network-state", 101)?;
    let scope = created_scope(&journal, &binding, SET_ID).await?;
    let request = scope
        .assignment_identity(AssignmentKey::AssignedRequest {
            request_id: REQUEST_ID,
        })
        .await?;
    let source = receipt(&scope, REQUEST_ID, 911, 101).await?;
    if scope.record_source_authorization(&request, &source).await?
        != SourceAuthorizationResult::Recorded
    {
        return Err(HostError::Journal);
    }

    let sample = "9".repeat(32);
    let permit = StartPermit::test_fixture_with_engine_id(&sample, TEST_ENGINE_ID);
    let docker_root_digest = permit.docker_root_digest().to_owned();
    let admission = fixture_storage_provisioning_admission(&permit)?;
    let physical = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: "a".repeat(32),
        })
        .await?;
    let RequestPhysicalReservationResult::AcquireClaimed {
        reservation_id,
        intent_id,
    } = scope
        .try_reserve_and_claim_request_acquire(
            ReservationRequest {
                assignment: &physical,
                engine_id: TEST_ENGINE_ID,
                docker_root_digest: &docker_root_digest,
                cpu_millicores: 2_000,
                memory_bytes: JOB_MEMORY,
                sample_generation_id: &sample,
            },
            permit,
            admission,
            &request,
        )
        .await?
    else {
        return Err(HostError::Journal);
    };

    insert_pending_network_row(&scope, reservation_id, intent_id, &docker_root_digest).await?;
    assert_eq!(
        scope.request_acquire_state(&request).await?,
        Some(RequestAcquireState::Requested)
    );
    assert!(
        !scope
            .record_request_acquire_and_settle_unused(&request, AssignedAcquireResult::NoEffect)
            .await?
    );
    assert_eq!(
        scope.request_acquire_state(&request).await?,
        Some(RequestAcquireState::NoEffect)
    );
    assert_eq!(
        scope.known_no_acquire_disposition(&request).await?,
        Some((RequestAcquireState::NoEffect, false))
    );
    assert_eq!(
        scope
            .reservation(&physical)
            .await?
            .map(|record| record.state),
        Some(crate::journal::ReservationState::Reserved)
    );
    assert_eq!(journal.occupied_reservation_count().await?, 1);
    Ok(())
}

async fn insert_pending_network_row(
    scope: &crate::journal::ConnectionJournal,
    reservation_id: i64,
    intent_id: i64,
    docker_root_digest: &str,
) -> Result<(), HostError> {
    let connection_id = scope.connection_id().as_str().to_owned();
    let engine_id = TEST_ENGINE_ID;
    let mut labels = BTreeMap::new();
    labels.insert("io.velnor.connection-id".to_owned(), connection_id.clone());
    labels.insert("io.velnor.engine-id".to_owned(), engine_id.to_owned());
    labels.insert("io.velnor.managed".to_owned(), "true".to_owned());
    labels.insert("io.velnor.operation-id".to_owned(), intent_id.to_string());
    labels.insert(
        "io.velnor.reservation-id".to_owned(),
        reservation_id.to_string(),
    );
    let labels_json = serde_json::to_string(&labels).map_err(|_| HostError::Journal)?;
    let network_name = format!("velnor-v{}-private", "a".repeat(32));
    crate::journal::connection::storage::in_transaction(scope.journal_ref(), async |connection| {
        connection
            .execute(
                "INSERT INTO capacity_reservation_networks (reservation_id, connection_id, operation_id, engine_id, docker_root_digest, network_name, labels_json, create_state, remove_state) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'requested', 'not_requested')",
                (
                    reservation_id,
                    connection_id.as_str(),
                    intent_id,
                    engine_id,
                    docker_root_digest,
                    network_name.as_str(),
                    labels_json.as_str(),
                ),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        Ok(())
    })
    .await
}
