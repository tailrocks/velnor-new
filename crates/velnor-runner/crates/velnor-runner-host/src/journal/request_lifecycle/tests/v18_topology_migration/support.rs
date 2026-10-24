use super::{StorageChargeTotals, V18_PUBLICATION_DDL};
use crate::error::HostError;

pub(super) async fn query_storage_charge_totals(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<StorageChargeTotals, HostError> {
    let mut rows = connection
        .query(
            "SELECT COUNT(*), COALESCE(SUM(charge_bytes), 0), COALESCE(SUM(charge_inodes), 0), COALESCE(SUM(retained_memory_bytes), 0), (SELECT COUNT(*) FROM capacity_reservation_provisioning_pools WHERE reservation_id = ?1) FROM capacity_reservation_provisioning_roles WHERE reservation_id = ?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    Ok(StorageChargeTotals {
        role_count: row.get(0).map_err(|_| HostError::Journal)?,
        charge_bytes: row.get(1).map_err(|_| HostError::Journal)?,
        charge_inodes: row.get(2).map_err(|_| HostError::Journal)?,
        retained_memory_bytes: row.get(3).map_err(|_| HostError::Journal)?,
        pool_count: row.get(4).map_err(|_| HostError::Journal)?,
    })
}

pub(super) async fn scalar_i64(
    connection: &turso::Connection,
    sql: &str,
    reservation_id: i64,
) -> Result<i64, HostError> {
    let mut rows = connection
        .query(sql, [reservation_id])
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)
}

pub(in crate::journal::request_lifecycle::tests) fn install_v18_bound_fixture(
    path: &std::path::Path,
) -> Result<(), HostError> {
    const FIXTURE: &[u8] = include_bytes!("v18_bound_fixture.sqlite");
    std::fs::write(path, FIXTURE).map_err(|_| HostError::Journal)
}

pub(in crate::journal::request_lifecycle::tests) async fn assert_fixture_version_eighteen(
    path: &std::path::Path,
) -> Result<(), HostError> {
    let database = turso::Builder::new_local(path.to_str().ok_or(HostError::Path)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    assert_eq!(read_user_version(&connection).await?, 18);
    Ok(())
}

pub(super) async fn assert_version_eighteen_publication_key(
    path: &std::path::Path,
) -> Result<(), HostError> {
    let database = turso::Builder::new_local(path.to_str().ok_or(HostError::Path)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    assert_eq!(read_user_version(&connection).await?, 18);
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = 'capacity_reservation_provisioning_publications'",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let actual = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<String>(0)
        .map_err(|_| HostError::Journal)?;
    assert_eq!(
        canonical_table_sql(&actual),
        canonical_table_sql(V18_PUBLICATION_DDL)
    );
    assert_eq!(
        publication_primary_key(&connection).await?,
        ["reservation_id"]
    );
    Ok(())
}

pub(super) async fn replace_publication_key_with_unknown_shape(
    path: &std::path::Path,
) -> Result<(), HostError> {
    let database = turso::Builder::new_local(path.to_str().ok_or(HostError::Path)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    for trigger in [
        "provisioning_publication_no_update",
        "provisioning_publication_no_delete",
    ] {
        connection
            .execute(&format!("DROP TRIGGER IF EXISTS {trigger}"), ())
            .await
            .map_err(|error| panic!("unknown-key setup SQL failed: {error:?}"))?;
    }
    connection
        .execute(
            "ALTER TABLE capacity_reservation_provisioning_publications RENAME TO capacity_reservation_provisioning_publications_unknown",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "CREATE TABLE capacity_reservation_provisioning_publications AS SELECT * FROM capacity_reservation_provisioning_publications_unknown WHERE 0",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "DROP TABLE capacity_reservation_provisioning_publications_unknown",
            (),
        )
        .await
        .map_err(|error| panic!("unknown-key setup SQL failed: {error:?}"))?;
    Ok(())
}

pub(super) async fn read_user_version(connection: &turso::Connection) -> Result<i64, HostError> {
    let mut rows = connection
        .query("PRAGMA user_version", ())
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)
}

pub(super) async fn publication_primary_key(
    connection: &turso::Connection,
) -> Result<Vec<String>, HostError> {
    let mut rows = connection
        .query(
            "PRAGMA table_info(capacity_reservation_provisioning_publications)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut columns = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let ordinal = row.get::<i64>(5).map_err(|_| HostError::Journal)?;
        if ordinal > 0 {
            columns.push((
                ordinal,
                row.get::<String>(1).map_err(|_| HostError::Journal)?,
            ));
        }
    }
    columns.sort_unstable_by_key(|(ordinal, _)| *ordinal);
    Ok(columns.into_iter().map(|(_, name)| name).collect())
}

fn canonical_table_sql(sql: &str) -> String {
    let mut normalized = sql.to_ascii_lowercase().replace(char::is_whitespace, "");
    if normalized.starts_with("createtableifnotexists") {
        normalized.replace_range(.."createtableifnotexists".len(), "createtable");
    }
    normalized
}
