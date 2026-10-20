use super::*;
use crate::journal::{ProbePhase, ProbeSeed, ResourceProbePurpose};
use crate::launch::VerifiedProbeCleanup;
use sha2::{Digest, Sha256};
use velnor_resource_probe::cleanup_observer_wire::{
    CleanupObserverBinding, CleanupObserverRequest,
};

#[tokio::test]
async fn legacy_null_inode_witness_reopens_for_cleanup_and_keeps_capacity_held()
-> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    configure_cleanup_capacity(&journal).await?;
    let repository = binding("cleanup-identity", 1_401)?;
    let scope = created_scope(&journal, &repository, 2_401).await?;
    let unbound = prepare_physical_initializer(&scope, 'c').await?;
    assert!(scope.storage_cleanup_identity(unbound).await?.is_none());
    let provisioned = provision_physical_intent(&scope, 'd').await?;
    make_legacy_witness(&journal, provisioned.reservation_id).await?;
    drop(scope);
    drop(journal);

    let reopened = Journal::open(&scratch.file()).await?;
    configure_cleanup_capacity(&reopened).await?;
    let scope = reopened.connection_scope(&repository).await?;
    let cleanup = scope
        .storage_cleanup_identity(provisioned.reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(cleanup.reservation_id(), provisioned.reservation_id);
    run_legacy_cleanup_probe(&reopened, &scope, &provisioned).await?;
    assert_capacity_remains_held(&reopened, provisioned.reservation_id).await
}

async fn configure_cleanup_capacity(journal: &Journal) -> Result<(), HostError> {
    journal
        .configure_capacity_with_operational_limits(
            crate::journal::CapacityLimits {
                jobs: 2,
                cpu_millicores: JOB_LIMIT.cpu_millicores * 2,
                memory_bytes: 40 * 1024 * 1024 * 1024,
            },
            crate::journal::OperationalCapacityLimits {
                cpu_millicores: 100,
                memory_bytes: 128 * 1024 * 1024,
            },
        )
        .await?;
    Ok(())
}

async fn make_legacy_witness(journal: &Journal, reservation_id: i64) -> Result<(), HostError> {
    let connection = journal.connection().await?;
    let json = read_binding_json(&connection, reservation_id).await?;
    let digest = digest_hex(json.as_bytes());
    connection
        .execute("DROP TRIGGER cleanup_observer_witness_no_update", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "UPDATE capacity_reservation_cleanup_observer_witnesses SET binding_json = ?1, binding_sha256 = ?2 WHERE reservation_id = ?3",
            (json, digest, reservation_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute("DROP TRIGGER bound_storage_path_no_update", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "UPDATE capacity_reservation_bound_storage_paths SET statx_inode = NULL WHERE reservation_id = ?1 AND role = 'engine-metadata'",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    restore_legacy_guards(&connection).await
}

async fn restore_legacy_guards(connection: &turso::Connection) -> Result<(), HostError> {
    connection
        .execute(
            "CREATE TRIGGER bound_storage_path_no_update BEFORE UPDATE ON capacity_reservation_bound_storage_paths BEGIN SELECT RAISE(ABORT, 'The observed storage path cannot change.'); END",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "CREATE TRIGGER cleanup_observer_witness_no_update BEFORE UPDATE ON capacity_reservation_cleanup_observer_witnesses BEGIN SELECT RAISE(ABORT, 'cleanup observer witness is immutable'); END",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn read_binding_json(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<String, HostError> {
    let mut rows = connection
        .query(
            "SELECT binding_json FROM capacity_reservation_cleanup_observer_witnesses WHERE reservation_id = ?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let json = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<String>(0)
        .map_err(|_| HostError::Journal)?;
    let mut binding: CleanupObserverBinding =
        serde_json::from_str(&json).map_err(|_| HostError::Journal)?;
    binding.metadata_job_facts_inode = None;
    serde_json::to_string(&binding).map_err(|_| HostError::Journal)
}

async fn run_legacy_cleanup_probe(
    journal: &Journal,
    scope: &crate::journal::ConnectionJournal,
    provisioned: &super::super::helpers::ProvisionedRunner,
) -> Result<(), HostError> {
    let witness = scope
        .storage_cleanup_witness(provisioned.reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    assert!(witness.metadata_job_facts_inode().is_none());
    let operation_id = "e".repeat(32);
    let request = CleanupObserverRequest {
        schema_version: 1,
        operation_id: operation_id.clone(),
        binding_sha256: witness.binding_sha256().to_owned(),
        binding: witness.binding().clone(),
    };
    let request_bytes = serde_json::to_vec(&request).map_err(|_| HostError::Journal)?;
    let operation_name = format!("velnor-resource-probe-{operation_id}");
    let seed = cleanup_seed(
        journal,
        provisioned,
        operation_id.clone(),
        operation_name.clone(),
        &request_bytes,
    )
    .await?;
    let prepared = journal
        .prepare_cleanup_observer_probe(scope, provisioned.reservation_id, seed)
        .await?;
    assert!(
        prepared
            .request()
            .binding
            .metadata_job_facts_inode
            .is_none()
    );
    retire_test_cleanup_probe(journal, provisioned, operation_id, operation_name).await?;
    super::super::helpers::record_owned_storage_cleanup_proof(scope, provisioned).await
}

async fn cleanup_seed(
    journal: &Journal,
    provisioned: &super::super::helpers::ProvisionedRunner,
    operation_id: String,
    operation_name: String,
    request_bytes: &[u8],
) -> Result<ProbeSeed, HostError> {
    Ok(ProbeSeed {
        operation_id,
        instance_id: journal.instance_id().await?,
        engine_id: super::super::helpers::TEST_ENGINE_ID.to_owned(),
        docker_root_digest: provisioned.docker_root_digest.clone(),
        source_revision: "b".repeat(40),
        runtime_image_id: format!("sha256:{}", "c".repeat(64)),
        image_binding_digest: "d".repeat(64),
        operation_name,
        projection_digest: "f".repeat(64),
        purpose: ResourceProbePurpose::CleanupObserver,
        request_sha256: cleanup_request_digest(request_bytes),
    })
}

async fn retire_test_cleanup_probe(
    journal: &Journal,
    provisioned: &super::super::helpers::ProvisionedRunner,
    operation_id: String,
    operation_name: String,
) -> Result<(), HostError> {
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
            super::super::helpers::TEST_ENGINE_ID.to_owned(),
            provisioned.docker_root_digest.clone(),
            ProbePhase::RemoveRequested,
            operation_name,
            Some(helper_id),
        ))
        .await
}

async fn assert_capacity_remains_held(
    journal: &Journal,
    reservation_id: i64,
) -> Result<(), HostError> {
    let query = journal.connection().await?;
    let mut rows = query
        .query(
            "SELECT state, cpu_millicores, memory_bytes FROM capacity_reservations WHERE id = ?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    assert_eq!(
        row.get::<String>(0).map_err(|_| HostError::Journal)?,
        "reserved"
    );
    assert_eq!(row.get::<i64>(1).map_err(|_| HostError::Journal)?, 2_000);
    assert_eq!(
        row.get::<i64>(2).map_err(|_| HostError::Journal)?,
        8_589_934_592
    );
    Ok(())
}

fn cleanup_request_digest(value: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"velnor-cleanup-observer-request-v1\0");
    hasher.update(value);
    hasher.update(b"\n");
    encode_hex(&hasher.finalize())
}

fn digest_hex(value: &[u8]) -> String {
    encode_hex(&Sha256::digest(value))
}

fn encode_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(result, "{byte:02x}");
    }
    result
}
