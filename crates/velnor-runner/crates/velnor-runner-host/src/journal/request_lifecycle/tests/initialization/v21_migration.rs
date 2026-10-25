use crate::error::HostError;
use crate::journal::Journal;

use super::super::helpers::Scratch;
#[tokio::test]
async fn version_twenty_one_migration_keeps_two_volume_rows_recovery_only() -> Result<(), HostError>
{
    let scratch = Scratch::new()?;
    let path = scratch.file();
    std::fs::write(&path, include_bytes!("v21_fixture.sqlite")).map_err(|_| HostError::Journal)?;
    let database = turso::Builder::new_local(path.to_str().ok_or(HostError::Path)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    assert_eq!(read_user_version(&connection).await?, 21);
    drop(connection);
    drop(database);

    let journal = Journal::open(&path).await?;
    let database = turso::Builder::new_local(path.to_str().ok_or(HostError::Path)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    let mut rows = connection
        .query(
            "SELECT layout_version, metadata_volume, metadata_volume_id, metadata_layout_digest, result_state, remove_state FROM capacity_reservation_storage_initialization WHERE reservation_id = ?1",
            [1_i64],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    assert_eq!(row.get::<i64>(0).map_err(|_| HostError::Journal)?, 1);
    for index in [1, 2, 3] {
        assert!(
            row.get::<Option<String>>(index)
                .map_err(|_| HostError::Journal)?
                .is_none()
        );
    }
    assert_eq!(
        row.get::<String>(4).map_err(|_| HostError::Journal)?,
        "confirmed"
    );
    assert_eq!(
        row.get::<String>(5).map_err(|_| HostError::Journal)?,
        "removed"
    );
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM capacity_reservation_root_backing_mappings WHERE reservation_id = ?1",
            [1_i64],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    assert_eq!(
        rows.next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?
            .get::<i64>(0)
            .map_err(|_| HostError::Journal)?,
        0
    );
    drop(rows);
    drop(connection);
    drop(database);
    drop(journal);
    Ok(())
}

async fn read_user_version(connection: &turso::Connection) -> Result<i64, HostError> {
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
