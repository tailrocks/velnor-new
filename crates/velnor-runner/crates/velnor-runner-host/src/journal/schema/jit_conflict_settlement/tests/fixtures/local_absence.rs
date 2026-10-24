use crate::error::HostError;
use crate::journal::request_lifecycle::tests::helpers::{
    Scratch, TEST_ENGINE_ID, binding, created_scope, provision_acquired_request_with_initialization,
};
use crate::journal::{
    CapacityLimits, Journal, OperationalCapacityLimits, ProbePhase, ProbeSeed, ResourceProbePurpose,
};
use crate::launch::VerifiedProbeCleanup;
use sha2::{Digest, Sha256};
use velnor_resource_probe::cleanup_observer_wire::CleanupObserverRequest;

use super::{LocalAbsenceFixture, record_local_absence};

pub(super) async fn record_cleanup_observer_probe(
    journal: &Journal,
    scope: &crate::journal::ConnectionJournal,
    physical: &crate::journal::request_lifecycle::tests::helpers::JobFactsReaderFixture,
    nonce: char,
) -> Result<(), HostError> {
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
    let witness = scope
        .storage_cleanup_witness(physical.reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    let request = CleanupObserverRequest {
        schema_version: 1,
        operation_id: operation_id.clone(),
        binding_sha256: witness.binding_sha256().to_owned(),
        binding: witness.binding().clone(),
    };
    let request_json = serde_json::to_vec(&request).map_err(|_| HostError::Journal)?;
    let mut digest = Sha256::new();
    digest.update(b"velnor-cleanup-observer-request-v1\0");
    digest.update(&request_json);
    digest.update(b"\n");
    let seed = ProbeSeed {
        operation_id: operation_id.clone(),
        instance_id: journal.instance_id().await?,
        engine_id: TEST_ENGINE_ID.to_owned(),
        docker_root_digest: physical.docker_root_digest.clone(),
        source_revision: "b".repeat(40),
        runtime_image_id: format!("sha256:{}", "c".repeat(64)),
        image_binding_digest: "d".repeat(64),
        operation_name: operation_name.clone(),
        projection_digest: "e".repeat(64),
        purpose: ResourceProbePurpose::CleanupObserver,
        request_sha256: crate::digest_hex::lowercase_hex(&digest.finalize()),
    };
    journal
        .prepare_cleanup_observer_probe(scope, physical.reservation_id, seed)
        .await?;
    journal
        .transition_probe(
            &operation_id,
            ProbePhase::Prepared,
            ProbePhase::CreateRequested,
        )
        .await?;
    let helper_id = "a".repeat(64);
    journal
        .bind_probe_container(&operation_id, &helper_id)
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
            physical.docker_root_digest.clone(),
            ProbePhase::RemoveRequested,
            operation_name,
            Some(helper_id),
        ))
        .await
}

pub(in crate::journal::schema::jit_conflict_settlement) async fn local_absence_fixture(
    nonce: char,
    set_id: i64,
    request_id: i64,
) -> Result<LocalAbsenceFixture, HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    finish_local_absence_fixture(scratch, journal, nonce, set_id, request_id).await
}

#[inline(never)]
fn finish_local_absence_fixture(
    scratch: Scratch,
    journal: Journal,
    nonce: char,
    set_id: i64,
    request_id: i64,
) -> impl std::future::Future<Output = Result<LocalAbsenceFixture, HostError>> + Send {
    Box::pin(async move {
        journal
            .configure_capacity(CapacityLimits {
                jobs: 1,
                cpu_millicores: 2_000,
                memory_bytes: 25_769_803_776,
            })
            .await?;
        let repository = binding(&format!("jit-v40-null-{nonce}"), 101)?;
        let scope = created_scope(&journal, &repository, set_id).await?;
        let physical =
            provision_acquired_request_with_initialization(&scope, request_id, nonce, nonce)
                .await?;
        let requested_name = format!("v{}", nonce.to_string().repeat(32));
        let (authority_generation, authority_digest) =
            scope.test_active_authority_identity().await?;
        let jit_claim = scope
            .claim_runner_jit_for_request(
                physical.intent_id,
                physical.reservation_id,
                request_id,
                &requested_name,
                authority_generation,
                &authority_digest,
            )
            .await?
            .ok_or(HostError::Journal)?;
        scope
            .mark_runner_jit_conflict_received(
                jit_claim,
                physical.reservation_id,
                authority_generation,
                &authority_digest,
            )
            .await?;
        let candidate = scope
            .recoverable_jit_conflict_candidates()
            .await?
            .into_iter()
            .find(|candidate| candidate.reservation_id() == physical.reservation_id)
            .ok_or(HostError::Journal)?;
        let recovery_claim = scope
            .claim_jit_conflict_candidate(&candidate, 180)
            .await?
            .ok_or(HostError::Journal)?;

        record_local_absence(&scope, &recovery_claim, &physical, nonce).await?;
        Ok(LocalAbsenceFixture {
            scratch,
            journal,
            scope,
            recovery_claim,
            physical,
            requested_name,
        })
    })
}
