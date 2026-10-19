use crate::error::HostError;
use crate::journal::{AssignmentKey, Journal, ReservationState, StorageRole};

use super::helpers::{Scratch, binding};

const MEMORY_BYTES: u64 = 8_589_934_592;

// Exact v18 source: frozen C27 `journal/schema/provisioning/ddl.rs` (SHA256
// ccc94ccde7c4dca5da099e46f52361873d9d7b27f76bd1005649f670a890e29c), table entry
// `capacity_reservation_provisioning_publications`. Its target is job-facts and its
// inline primary key is `reservation_id`; no second target is inferred here.
const V18_PUBLICATION_DDL: &str = "CREATE TABLE IF NOT EXISTS capacity_reservation_provisioning_publications (reservation_id INTEGER NOT NULL PRIMARY KEY REFERENCES capacity_reservation_provisioning(reservation_id), runner_id INTEGER NOT NULL CHECK (runner_id > 0), runner_name TEXT NOT NULL CHECK (length(runner_name) BETWEEN 1 AND 128), runner_scale_set_id INTEGER NOT NULL CHECK (runner_scale_set_id > 0), target TEXT NOT NULL CHECK (target = '/opt/velnor/job-facts/storage-cache-admission.json'), projection_sha256 TEXT NOT NULL CHECK (length(projection_sha256) = 64 AND projection_sha256 NOT GLOB '*[^0-9a-f]*'), directory_device_major INTEGER NOT NULL CHECK (directory_device_major >= 0), directory_device_minor INTEGER NOT NULL CHECK (directory_device_minor >= 0), directory_inode INTEGER NOT NULL CHECK (directory_inode > 0), mount_id INTEGER NOT NULL CHECK (mount_id > 0), v16_filesystem_id_0 INTEGER, v16_filesystem_id_1 INTEGER, filesystem_id TEXT CHECK (filesystem_id IS NULL OR (length(filesystem_id) = 16 AND filesystem_id NOT GLOB '*[^0-9a-f]*')), filesystem_instance_digest TEXT NOT NULL CHECK (length(filesystem_instance_digest) = 64 AND filesystem_instance_digest NOT GLOB '*[^0-9a-f]*'), read_only INTEGER NOT NULL CHECK (read_only = 1), CHECK ((v16_filesystem_id_0 IS NOT NULL AND v16_filesystem_id_1 IS NOT NULL AND filesystem_id IS NULL) OR (v16_filesystem_id_0 IS NULL AND v16_filesystem_id_1 IS NULL AND filesystem_id IS NOT NULL)))";

#[tokio::test]
async fn version_eighteen_charged_hold_migrates_as_legacy_and_stays_closed() -> Result<(), HostError>
{
    let scratch = Scratch::new()?;
    let path = scratch.file();
    install_v18_fixture(&path)?;
    assert_version_eighteen_publication_key(&path).await?;
    let fixture_database = turso::Builder::new_local(path.to_str().ok_or(HostError::Path)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let fixture_connection = fixture_database.connect().map_err(|_| HostError::Journal)?;
    let charged = query_storage_charge_totals(&fixture_connection, 1).await?;
    drop(fixture_connection);
    drop(fixture_database);

    let binding = binding("topology-migration", 303)?;
    let journal = Journal::open(&path).await?;
    let scope = journal.connection_scope(&binding).await?;
    let assignment = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: "22222222222222222222222222222222".to_owned(),
        })
        .await?;
    let reservation_id = 1;
    let before = scope
        .reservation(&assignment)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(before.state, ReservationState::Reserved);
    assert_eq!(before.cpu_millicores, 2_000);
    assert_eq!(before.memory_bytes, MEMORY_BYTES);
    drop(scope);
    drop(journal);

    let migrated = Journal::open(&path).await?;
    assert_legacy_topology(&migrated, &binding, &assignment, reservation_id, charged).await?;
    let scope = migrated.connection_scope(&binding).await?;
    assert!(
        scope
            .claim_storage_object_create(reservation_id, StorageRole::SharedRun)
            .await?
            .is_none()
    );
    assert!(
        scope
            .claim_bootstrap_container_create(
                reservation_id,
                crate::launch::bootstrap::BootstrapContainerRole::Runner,
            )
            .await?
            .is_none()
    );
    assert!(scope.claim_bootstrap_start(reservation_id).await?.is_none());
    assert!(
        scope
            .claim_job_facts_publication(reservation_id)
            .await?
            .is_none()
    );
    drop(scope);
    drop(migrated);

    let reopened = Journal::open(&path).await?;
    assert_legacy_topology(&reopened, &binding, &assignment, reservation_id, charged).await?;
    Ok(())
}

#[tokio::test]
async fn version_eighteen_unknown_publication_key_is_rejected() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    install_v18_fixture(&path)?;
    replace_publication_key_with_unknown_shape(&path)
        .await
        .expect("replace key");

    assert!(
        Journal::open(&path).await.is_err(),
        "The journal must reject an unknown key."
    );
    let database = turso::Builder::new_local(path.to_str().ok_or(HostError::Path)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    assert_eq!(read_user_version(&connection).await?, 18);
    assert_eq!(
        publication_primary_key(&connection).await?,
        Vec::<String>::new()
    );
    Ok(())
}

fn install_v18_fixture(path: &std::path::Path) -> Result<(), HostError> {
    const FIXTURE: &[u8] = include_bytes!("v18_topology_migration/v18_fixture.sqlite");
    std::fs::write(path, FIXTURE).map_err(|_| HostError::Journal)
}

async fn assert_legacy_topology(
    journal: &Journal,
    binding: &crate::config::RepositoryConnection,
    assignment: &crate::journal::AssignmentIdentity,
    reservation_id: i64,
    charged: StorageChargeTotals,
) -> Result<(), HostError> {
    let reservation = journal
        .connection_scope(binding)
        .await?
        .reservation(assignment)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(reservation.reservation_id, reservation_id);
    assert_eq!(reservation.state, ReservationState::Reserved);
    assert_eq!(reservation.cpu_millicores, 2_000);
    assert_eq!(reservation.memory_bytes, MEMORY_BYTES);
    assert_eq!(
        storage_charge_totals(journal, reservation_id).await?,
        charged
    );

    let connection = journal.connection().await?;
    assert_eq!(
        scalar_i64(
            &connection,
            "SELECT topology_version FROM capacity_reservation_provisioning_topology WHERE reservation_id = ?1",
            reservation_id,
        )
        .await?,
        1
    );
    assert_eq!(
        scalar_i64(
            &connection,
            "SELECT COUNT(*) FROM capacity_reservation_provisioning_objects WHERE reservation_id = ?1",
            reservation_id,
        )
        .await?,
        4
    );
    assert_eq!(
        scalar_i64(
            &connection,
            "SELECT COUNT(*) FROM capacity_reservation_provisioning_objects WHERE reservation_id = ?1 AND role = 'engine-metadata'",
            reservation_id,
        )
        .await?,
        0
    );
    assert_eq!(
        scalar_i64(
            &connection,
            "SELECT COUNT(*) FROM capacity_reservation_provisioning_publications WHERE reservation_id = ?1",
            reservation_id,
        )
        .await?,
        0
    );
    assert_eq!(charged.role_count, 11);
    assert_eq!(charged.pool_count, 9);
    assert!(charged.charge_bytes > 0);
    assert!(charged.charge_inodes > 0);
    assert!(charged.retained_memory_bytes > 0);
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct StorageChargeTotals {
    role_count: i64,
    pool_count: i64,
    charge_bytes: i64,
    charge_inodes: i64,
    retained_memory_bytes: i64,
}

async fn storage_charge_totals(
    journal: &Journal,
    reservation_id: i64,
) -> Result<StorageChargeTotals, HostError> {
    let connection = journal.connection().await?;
    query_storage_charge_totals(&connection, reservation_id).await
}

pub(super) mod support;
use support::{
    assert_version_eighteen_publication_key, publication_primary_key, query_storage_charge_totals,
    read_user_version, replace_publication_key_with_unknown_shape, scalar_i64,
};
