use crate::error::HostError;
use crate::journal::{
    HostHelperRole, Journal, ProbeLeaseState, ProbePhase, ProbeRow, ProbeWork, ResourceProbePurpose,
};

use super::{Scratch, seed, v11_fixture};

const LEGACY_CAPACITY_REQUEST_SHA256: &str =
    "5d76d86293261804706a16a73e0491cf4caa859f1b876cda706e568592b85cdf";

async fn update_request_digest(
    path: &std::path::Path,
    operation_id: &str,
    digest: &str,
) -> Result<(), HostError> {
    let path = path.to_str().ok_or(HostError::Path)?;
    let database = turso::Builder::new_local(path)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "UPDATE resource_probe_operations SET request_sha256 = ?1 WHERE operation_id = ?2",
            (digest, operation_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn read_request_digest(
    path: &std::path::Path,
    operation_id: &str,
) -> Result<String, HostError> {
    let path = path.to_str().ok_or(HostError::Path)?;
    let database = turso::Builder::new_local(path)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    let mut rows = connection
        .query(
            "SELECT request_sha256 FROM resource_probe_operations WHERE operation_id = ?1",
            (operation_id,),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows.next().await.map_err(|_| HostError::Journal)?;
    row.ok_or(HostError::Journal)?
        .get::<String>(0)
        .map_err(|_| HostError::Journal)
}

fn assert_migrated_probe(row: &ProbeRow) {
    assert_eq!(row.operation_id, v11_fixture::OPERATION_ID);
    assert_eq!(row.instance_id, v11_fixture::INSTANCE_ID);
    assert_eq!(row.engine_id, v11_fixture::ENGINE_ID);
    assert_eq!(row.docker_root_digest, v11_fixture::DOCKER_ROOT_DIGEST);
    assert_eq!(row.source_revision, "b".repeat(40));
    assert_eq!(row.runtime_image_id, format!("sha256:{}", "c".repeat(64)));
    assert_eq!(row.image_binding_digest, "d".repeat(64));
    assert_eq!(row.operation_name, v11_fixture::OPERATION_NAME);
    assert_eq!(row.role, "resource-probe");
    assert_eq!(row.helper_role(), HostHelperRole::ResourceProbe);
    assert_eq!(
        row.work(),
        &ProbeWork::ResourceProbe {
            purpose: ResourceProbePurpose::Capacity,
            request_sha256: crate::journal::capacity_request_sha256(),
            request_context: None,
        }
    );
    assert_eq!(row.projection_digest, "e".repeat(64));
    assert_eq!(row.phase, ProbePhase::CreateRequested);
    assert_eq!(row.container_id, None);
    assert_eq!(row.lease_state, ProbeLeaseState::Uncertain);
}

async fn assert_migrated_lease(journal: &Journal) -> Result<(), HostError> {
    let connection = journal.connection().await?;
    let mut leases = connection
        .query(
            "SELECT operation_id, engine_id, docker_root_digest, operation_name, container_id, cpu_millicores, memory_bytes, state FROM host_operation_leases WHERE singleton = 1",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let lease = leases.next().await.map_err(|_| HostError::Journal)?;
    let lease = lease.ok_or(HostError::Journal)?;
    assert_eq!(
        lease.get::<String>(0).map_err(|_| HostError::Journal)?,
        v11_fixture::OPERATION_ID
    );
    assert_eq!(
        lease.get::<String>(1).map_err(|_| HostError::Journal)?,
        v11_fixture::ENGINE_ID
    );
    assert_eq!(
        lease.get::<String>(2).map_err(|_| HostError::Journal)?,
        v11_fixture::DOCKER_ROOT_DIGEST
    );
    assert_eq!(
        lease.get::<String>(3).map_err(|_| HostError::Journal)?,
        v11_fixture::OPERATION_NAME
    );
    assert_eq!(
        lease
            .get::<Option<String>>(4)
            .map_err(|_| HostError::Journal)?,
        None
    );
    assert_eq!(
        lease.get::<i64>(5).map_err(|_| HostError::Journal)?,
        i64::try_from(crate::worker::PROBE_CPU_MILLICORES).map_err(|_| HostError::Journal)?
    );
    assert_eq!(
        lease.get::<i64>(6).map_err(|_| HostError::Journal)?,
        i64::try_from(crate::worker::PROBE_MEMORY_LIMIT_BYTES).map_err(|_| HostError::Journal)?
    );
    assert_eq!(
        lease.get::<String>(7).map_err(|_| HostError::Journal)?,
        "uncertain"
    );
    assert!(
        leases
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .is_none()
    );
    Ok(())
}

#[tokio::test]
async fn version_eleven_migration_backfills_an_uncertain_probe_lease() -> Result<(), HostError> {
    let scratch = Scratch::new();
    let path = scratch.file();
    v11_fixture::seed(&path).await?;

    let migrated = Journal::open(&path).await?;
    let row = migrated.active_probe().await?.ok_or(HostError::Journal)?;
    assert_migrated_probe(&row);
    assert_migrated_lease(&migrated).await?;
    assert!(!migrated.is_drained().await?);
    assert!(
        migrated
            .prepare_probe(seed(&migrated, "e").await?)
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn migration_repairs_only_the_known_legacy_capacity_request_digest() -> Result<(), HostError>
{
    let scratch = Scratch::new();
    let path = scratch.file();
    v11_fixture::seed(&path).await?;

    let migrated = Journal::open(&path).await?;
    assert!(migrated.active_probe().await?.is_some());
    drop(migrated);
    update_request_digest(
        &path,
        v11_fixture::OPERATION_ID,
        LEGACY_CAPACITY_REQUEST_SHA256,
    )
    .await?;
    assert_eq!(
        read_request_digest(&path, v11_fixture::OPERATION_ID).await?,
        LEGACY_CAPACITY_REQUEST_SHA256
    );

    let reopened = Journal::open(&path).await?;
    let row = reopened.active_probe().await?.ok_or(HostError::Journal)?;
    assert_eq!(
        row.work(),
        &ProbeWork::ResourceProbe {
            purpose: ResourceProbePurpose::Capacity,
            request_sha256: crate::journal::capacity_request_sha256(),
            request_context: None,
        }
    );
    Ok(())
}

#[tokio::test]
async fn migration_does_not_rewrite_an_unrecognized_capacity_request_digest()
-> Result<(), HostError> {
    let scratch = Scratch::new();
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    super::setup(&journal, 100, 128 * 1024 * 1024).await?;
    journal.prepare_probe(seed(&journal, "a").await?).await?;
    drop(journal);

    let unrelated_digest = "f".repeat(64);
    let operation_id = "a".repeat(32);
    update_request_digest(&path, &operation_id, &unrelated_digest).await?;

    let reopened = Journal::open(&path).await?;
    assert!(reopened.active_probe().await.is_err());
    drop(reopened);
    assert_eq!(
        read_request_digest(&path, &operation_id).await?,
        unrelated_digest
    );
    Ok(())
}
