use crate::error::HostError;
use crate::journal::request_lifecycle::tests::helpers::{
    Scratch, binding, test_initializer_limits,
};
use crate::journal::request_lifecycle::tests::v18_topology_migration::support::install_v18_bound_fixture;
use crate::journal::{AssignmentKey, Journal, ReservationState};

const RESERVATION_ID: i64 = 1;
const MEMORY_BYTES: u64 = 8_589_934_592;
const RUNNER_SUBJECT: &str = "22222222222222222222222222222222";

#[tokio::test]
async fn migrated_active_reservation_cannot_claim_a_late_initializer() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    install_v18_bound_fixture(&path)?;
    super::super::v18_topology_migration::support::assert_fixture_version_eighteen(&path).await?;
    let original_charge = read_charge_snapshot(&path).await?;
    assert_eq!(original_charge.roles, 11);
    assert_eq!(original_charge.pools, 9);
    assert_eq!(original_charge.objects, 4);
    assert!(original_charge.bytes > 0);
    assert!(original_charge.inodes > 0);
    assert!(original_charge.retained_memory > 0);

    let binding = binding("topology-migration", 303)?;
    let journal = Journal::open(&path).await?;
    let scope = journal.connection_scope(&binding).await?;
    let assignment = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: RUNNER_SUBJECT.to_owned(),
        })
        .await?;
    assert_reserved_identity(&scope, &assignment).await?;
    assert_late_claim_is_refused(&scope).await?;
    assert_eq!(
        read_charge_snapshot_connection(&journal).await?,
        original_charge
    );
    drop(scope);
    drop(journal);

    let reopened = Journal::open(&path).await?;
    let reopened_scope = reopened.connection_scope(&binding).await?;
    let reopened_assignment = reopened_scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: RUNNER_SUBJECT.to_owned(),
        })
        .await?;
    assert_reserved_identity(&reopened_scope, &reopened_assignment).await?;
    assert_eq!(
        read_charge_snapshot_connection(&reopened).await?,
        original_charge
    );
    Ok(())
}

async fn assert_reserved_identity(
    scope: &crate::journal::ConnectionJournal,
    assignment: &crate::journal::AssignmentIdentity,
) -> Result<(), HostError> {
    let reservation = scope
        .reservation(assignment)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(reservation.reservation_id, RESERVATION_ID);
    assert_eq!(reservation.state, ReservationState::Reserved);
    assert_eq!(reservation.cpu_millicores, 2_000);
    assert_eq!(reservation.memory_bytes, MEMORY_BYTES);
    Ok(())
}

async fn assert_late_claim_is_refused(
    scope: &crate::journal::ConnectionJournal,
) -> Result<(), HostError> {
    let image = crate::worker::storage_initialization::specification::test_verified_image()?;
    assert!(
        scope
            .claim_storage_initialization(RESERVATION_ID, &image, test_initializer_limits()?)
            .await?
            .is_none(),
        "a reservation migrated from v18 cannot acquire a new initializer"
    );
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ChargeSnapshot {
    roles: i64,
    pools: i64,
    bytes: i64,
    inodes: i64,
    retained_memory: i64,
    objects: i64,
}

async fn read_charge_snapshot(path: &std::path::Path) -> Result<ChargeSnapshot, HostError> {
    let database = turso::Builder::new_local(path.to_str().ok_or(HostError::Path)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    read_charge_snapshot_on(&connection).await
}

async fn read_charge_snapshot_connection(journal: &Journal) -> Result<ChargeSnapshot, HostError> {
    let connection = journal.connection().await?;
    read_charge_snapshot_on(&connection).await
}

async fn read_charge_snapshot_on(
    connection: &turso::Connection,
) -> Result<ChargeSnapshot, HostError> {
    let mut rows = connection
        .query(
            "SELECT (SELECT COUNT(*) FROM capacity_reservation_provisioning_roles WHERE reservation_id = ?1), (SELECT COUNT(*) FROM capacity_reservation_provisioning_pools WHERE reservation_id = ?1), (SELECT COALESCE(SUM(charge_bytes), 0) FROM capacity_reservation_provisioning_roles WHERE reservation_id = ?1), (SELECT COALESCE(SUM(charge_inodes), 0) FROM capacity_reservation_provisioning_roles WHERE reservation_id = ?1), (SELECT COALESCE(SUM(retained_memory_bytes), 0) FROM capacity_reservation_provisioning_roles WHERE reservation_id = ?1), (SELECT COUNT(*) FROM capacity_reservation_provisioning_objects WHERE reservation_id = ?1)",
            [RESERVATION_ID],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    Ok(ChargeSnapshot {
        roles: row.get(0).map_err(|_| HostError::Journal)?,
        pools: row.get(1).map_err(|_| HostError::Journal)?,
        bytes: row.get(2).map_err(|_| HostError::Journal)?,
        inodes: row.get(3).map_err(|_| HostError::Journal)?,
        retained_memory: row.get(4).map_err(|_| HostError::Journal)?,
        objects: row.get(5).map_err(|_| HostError::Journal)?,
    })
}
