use std::time::Instant;

use crate::error::HostError;
use crate::journal::request_lifecycle::tests::helpers::{Scratch, TEST_ENGINE_ID, binding};
use crate::journal::{
    CapacityLimits, Journal, MetadataSettlementResult, OperationalCapacityLimits, ProbePhase,
    ProbeSeed, ResourceProbePurpose, StorageAdmissionSample, StorageFilesystemKind, StorageMedium,
    StoragePoolObservation, StoragePoolObservationInput, StorageRole, StorageSampleIdentity,
    capacity_request_sha256,
};
use crate::launch::VerifiedProbeCleanup;
use crate::launch::bootstrap::{VerifiedStorageNetworkRemoved, test_verified_storage_cleanup};
use crate::worker::{WorkerIdentityNames, WorkerVolumeRole};
use velnor_runner_github::{
    Exchange, RunnerResponseReceipt, SessionRequest, Transport, TransportFail,
    observe_runner_by_name,
};

#[path = "fixtures/local_absence.rs"]
mod local_absence;

pub(in crate::journal::schema::jit_conflict_settlement) use local_absence::local_absence_fixture;

pub(in crate::journal::schema::jit_conflict_settlement) struct LocalAbsenceFixture {
    pub(super) scratch: Scratch,
    pub(super) journal: Journal,
    pub(super) scope: crate::journal::ConnectionJournal,
    pub(super) recovery_claim: crate::journal::connection::JitConflictRecoveryClaim,
    pub(super) physical: crate::journal::request_lifecycle::tests::helpers::JobFactsReaderFixture,
    pub(super) requested_name: String,
}

#[tokio::test]
async fn expired_claim_restarts_after_initial_absence_without_delete_tombstone()
-> Result<(), HostError> {
    let fixture = local_absence_fixture('7', 824, 1_124).await?;
    let first_observation = verified_empty_lookup(&fixture.requested_name)?;
    fixture
        .scope
        .record_jit_conflict_remote_initial_absence(&fixture.recovery_claim, &first_observation)
        .await?;

    let path = fixture.scratch.file();
    let reservation_id = fixture.physical.reservation_id;
    let first_generation = fixture.recovery_claim.claim_generation();
    let requested_name = fixture.requested_name.clone();
    let repository = binding("jit-v40-null-7", 101)?;
    expire_recovery_claim(&fixture, first_generation).await?;

    let LocalAbsenceFixture {
        scratch: _scratch,
        journal,
        scope,
        ..
    } = fixture;
    drop(scope);
    drop(journal);
    let reopened = Journal::open(&path).await?;
    let reopened_scope = reopened.connection_scope(&repository).await?;
    let candidate = reopened_scope
        .recoverable_jit_conflict_candidates()
        .await?
        .into_iter()
        .find(|candidate| candidate.reservation_id() == reservation_id)
        .ok_or(HostError::Journal)?;
    let retry_claim = reopened_scope
        .claim_jit_conflict_candidate(&candidate, 180)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(retry_claim.claim_generation(), first_generation + 1);
    assert!(
        reopened_scope
            .resume_jit_conflict_local_absence(&retry_claim)
            .await?
    );
    let second_observation = verified_empty_lookup(&requested_name)?;
    reopened_scope
        .record_jit_conflict_remote_initial_absence(&retry_claim, &second_observation)
        .await?;

    let connection = reopened.connection().await?;
    let mut rows = connection
        .query(
            "SELECT SUM(CASE WHEN phase_index=2 AND phase='remote_initial_absent' AND runner_id IS NULL AND lookup_status=200 AND lookup_count=0 THEN 1 ELSE 0 END),SUM(CASE WHEN phase_index=3 AND phase='delete_started' THEN 1 ELSE 0 END),MAX(claim_generation) FROM physical_attempt_jit_conflict_recovery_progress WHERE reservation_id=?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    assert_eq!(row.get::<i64>(0).map_err(|_| HostError::Journal)?, 2);
    assert_eq!(row.get::<i64>(1).map_err(|_| HostError::Journal)?, 0);
    assert_eq!(
        row.get::<i64>(2).map_err(|_| HostError::Journal)?,
        retry_claim.claim_generation()
    );
    super::super::super::validate_persisted_schema_v40(&connection).await?;
    Ok(())
}

pub(super) async fn expire_recovery_claim(
    fixture: &LocalAbsenceFixture,
    claim_generation: i64,
) -> Result<(), HostError> {
    let connection = fixture.journal.connection().await?;
    let claim_guard = super::super::schema_objects()
        .find(|(kind, name, _)| {
            *kind == "trigger" && *name == "physical_attempt_jit_conflict_claim_no_update"
        })
        .map(|(_, _, sql)| sql.to_owned())
        .ok_or(HostError::Journal)?;
    // Advance the lease clock deterministically, then restore the exact immutable-row guard.
    connection
        .execute(
            "DROP TRIGGER physical_attempt_jit_conflict_claim_no_update",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "UPDATE physical_attempt_jit_conflict_recovery_claims SET lease_until=unixepoch()-1 WHERE reservation_id=?1 AND claim_generation=?2",
            (fixture.physical.reservation_id, claim_generation),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(&claim_guard, ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(in crate::journal::schema::jit_conflict_settlement) fn verified_empty_lookup(
    requested_name: &str,
) -> Result<RunnerResponseReceipt, HostError> {
    let mut transport = EmptyLookupTransport;
    observe_runner_by_name(&mut transport, requested_name, "fixture-token")
        .map(|(_, receipt)| receipt)
        .map_err(|_| HostError::Journal)
}

pub(in crate::journal::schema::jit_conflict_settlement) async fn settle_empty_remote_case(
    fixture: &LocalAbsenceFixture,
    probe_nonce: char,
) -> Result<MetadataSettlementResult, HostError> {
    let initial = verified_empty_lookup(&fixture.requested_name)?;
    fixture
        .scope
        .record_jit_conflict_remote_initial_absence(&fixture.recovery_claim, &initial)
        .await?;
    let final_observation = verified_empty_lookup(&fixture.requested_name)?;
    fixture
        .scope
        .record_jit_conflict_remote_absence(&fixture.recovery_claim, &final_observation)
        .await?;

    record_network_removal(fixture).await?;
    let claim = settlement_probe(fixture, probe_nonce).await?;
    let sample = settlement_sample(&fixture.scope, &claim).await?;
    fixture
        .scope
        .record_storage_metadata_settlement(&claim, sample)
        .await
}

async fn record_network_removal(fixture: &LocalAbsenceFixture) -> Result<(), HostError> {
    let claim = fixture
        .scope
        .claim_storage_network_removal(fixture.physical.reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    let proof = VerifiedStorageNetworkRemoved::test_removed(&claim);
    fixture
        .scope
        .record_storage_network_removed(&claim, &proof)
        .await
}

async fn settlement_probe(
    fixture: &LocalAbsenceFixture,
    nonce: char,
) -> Result<crate::journal::StorageMetadataSettlementClaim, HostError> {
    let journal = &fixture.journal;
    journal
        .configure_capacity_with_operational_limits(
            CapacityLimits {
                jobs: 1,
                cpu_millicores: 2_000,
                memory_bytes: 25_769_803_776,
            },
            OperationalCapacityLimits {
                cpu_millicores: 1_000,
                memory_bytes: 128 * 1024 * 1024,
            },
        )
        .await?;
    let operation_id = nonce.to_string().repeat(32);
    let operation_name = format!("velnor-resource-probe-{operation_id}");
    let seed = ProbeSeed {
        operation_id: operation_id.clone(),
        instance_id: journal.instance_id().await?,
        engine_id: TEST_ENGINE_ID.to_owned(),
        docker_root_digest: fixture.physical.docker_root_digest.clone(),
        source_revision: "b".repeat(40),
        runtime_image_id: format!("sha256:{}", "c".repeat(64)),
        image_binding_digest: "d".repeat(64),
        operation_name: operation_name.clone(),
        projection_digest: "e".repeat(64),
        purpose: ResourceProbePurpose::Capacity,
        request_sha256: capacity_request_sha256(),
    };
    let claim = fixture
        .scope
        .prepare_storage_metadata_settlement_probe(fixture.physical.reservation_id, seed)
        .await?;
    journal
        .transition_probe(
            &operation_id,
            ProbePhase::Prepared,
            ProbePhase::CreateRequested,
        )
        .await?;
    let container_id = "f".repeat(64);
    journal
        .bind_probe_container(&operation_id, &container_id)
        .await?;
    journal
        .transition_probe(
            &operation_id,
            ProbePhase::ContainerCreated,
            ProbePhase::RemoveRequested,
        )
        .await?;
    journal
        .retire_probe(VerifiedProbeCleanup::test_only(
            operation_id,
            TEST_ENGINE_ID.to_owned(),
            fixture.physical.docker_root_digest.clone(),
            ProbePhase::RemoveRequested,
            operation_name,
            Some(container_id),
        ))
        .await?;
    Ok(claim)
}

async fn settlement_sample(
    scope: &crate::journal::ConnectionJournal,
    claim: &crate::journal::StorageMetadataSettlementClaim,
) -> Result<StorageAdmissionSample, HostError> {
    let profile = scope
        .current_storage_profile()
        .await?
        .ok_or(HostError::Journal)?;
    let disk = profile
        .pools()
        .iter()
        .find(|pool| pool.medium() == StorageMedium::Disk)
        .ok_or(HostError::Journal)?;
    let identity = StorageSampleIdentity::new(
        claim.probe_operation_id().to_owned(),
        claim.profile_generation_id().to_owned(),
        claim.profile_digest().to_owned(),
        claim.engine_id().to_owned(),
        claim.docker_root_digest().to_owned(),
    )?;
    let pool = StoragePoolObservation::new_with_filesystem(StoragePoolObservationInput {
        pool_id: disk.pool_id().to_owned(),
        filesystem_instance_digest: "a".repeat(64),
        medium: StorageMedium::Disk,
        filesystem_kind: StorageFilesystemKind::Btrfs,
        total_bytes: 64_u64 << 30,
        available_bytes: 60_u64 << 30,
        total_inodes: 0,
        available_inodes: 0,
    })?;
    StorageAdmissionSample::new(identity, Instant::now(), vec![pool], None)
}

struct EmptyLookupTransport;

impl Transport for EmptyLookupTransport {
    fn exchange(&mut self, _request: &SessionRequest) -> Result<Exchange, TransportFail> {
        Ok(Exchange {
            status: 200,
            body: br#"{"count":0,"value":[]}"#.to_vec(),
        })
    }
}

pub(super) async fn record_local_absence(
    scope: &crate::journal::ConnectionJournal,
    recovery_claim: &crate::journal::connection::JitConflictRecoveryClaim,
    physical: &crate::journal::request_lifecycle::tests::helpers::JobFactsReaderFixture,
    nonce: char,
) -> Result<(), HostError> {
    local_absence::record_cleanup_observer_probe(scope.journal_ref(), scope, physical, nonce)
        .await?;
    let names = WorkerIdentityNames::from_nonce(&nonce.to_string().repeat(32))?;
    let proof = test_verified_storage_cleanup(
        physical.reservation_id,
        physical.intent_id,
        TEST_ENGINE_ID.to_owned(),
        physical.docker_root_digest.clone(),
        physical.runner_container_id.clone(),
        physical.dind_container_id.clone(),
        vec![
            names.volume_name(WorkerVolumeRole::Socket),
            names.volume_name(WorkerVolumeRole::Home),
            names.volume_name(WorkerVolumeRole::Work),
            names.volume_name(WorkerVolumeRole::DindData),
            names.volume_name(WorkerVolumeRole::RunnerMetadata),
        ],
        physical.instance_binding_digest.clone(),
        StorageRole::PHYSICAL.to_vec(),
        true,
        true,
        true,
        true,
    )?;
    scope
        .record_jit_conflict_local_absence(recovery_claim, proof)
        .await
}

pub(super) async fn scalar(connection: &turso::Connection, sql: &str) -> Result<i64, HostError> {
    let mut rows = connection
        .query(sql, ())
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)
}
