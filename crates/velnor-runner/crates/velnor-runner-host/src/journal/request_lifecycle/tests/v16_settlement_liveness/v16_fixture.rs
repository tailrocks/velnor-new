//! This fixture checks the version-16 filesystem identity migration.
//! Its column order follows commit 9f67423d, file `schema/provisioning/migration.rs`.

use crate::error::HostError;

const HISTORICAL_V16_SQL: &str = include_str!("v16_fixture.sql");

pub(super) async fn preserve_filesystem_words(path: &std::path::Path) -> Result<(), HostError> {
    let path = path.to_str().ok_or(HostError::Path)?;
    let database = turso::Builder::new_local(path)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    for statement in HISTORICAL_V16_SQL.split(";\n").map(str::trim) {
        if !statement.is_empty() {
            connection
                .execute(statement, ())
                .await
                .map_err(|_| HostError::Journal)?;
        }
    }

    crate::journal::schema::provisioning::upgrade_v16_filesystem_ids_for_test(&connection).await?;
    for (table, key) in [
        (
            "capacity_reservation_bound_storage_attachments",
            "reservation_id",
        ),
        (
            "capacity_reservation_provisioning_publications",
            "reservation_id",
        ),
    ] {
        let mut rows = connection
            .query(
                &format!("SELECT filesystem_id, v16_filesystem_id_0, v16_filesystem_id_1 FROM {table} WHERE {key} = 1"),
                (),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let row = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?;
        assert_eq!(
            row.get::<Option<String>>(0)
                .map_err(|_| HostError::Journal)?,
            None
        );
        assert_eq!(row.get::<i64>(1).map_err(|_| HostError::Journal)?, 123);
        assert_eq!(row.get::<i64>(2).map_err(|_| HostError::Journal)?, 456);
        assert!(rows.next().await.map_err(|_| HostError::Journal)?.is_none());
    }
    Ok(())
}
