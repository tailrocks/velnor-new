use crate::error::HostError;

#[tokio::test]
async fn v27_validator_accepts_exact_v30_schema() -> Result<(), HostError> {
    let (_scratch, _journal, connection) = v30_connection().await?;
    super::super::validate_v27(&connection).await
}

#[tokio::test]
async fn v27_validator_rejects_drifted_v30_guard() -> Result<(), HostError> {
    let (_scratch, _journal, connection) = v30_connection().await?;
    connection
        .execute("DROP TRIGGER bound_storage_path_no_update", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "CREATE TRIGGER bound_storage_path_no_update BEFORE UPDATE ON capacity_reservation_bound_storage_paths BEGIN SELECT RAISE(ABORT, 'changed'); END",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    assert!(super::super::validate_v27(&connection).await.is_err());
    Ok(())
}

#[tokio::test]
async fn v27_validator_rejects_v30_schema_with_inode_guard() -> Result<(), HostError> {
    let (_scratch, _journal, connection) = v30_connection().await?;
    connection
        .execute(super::super::PATH_INODE_TRIGGER.1, ())
        .await
        .map_err(|_| HostError::Journal)?;
    assert!(super::super::validate_v27(&connection).await.is_err());
    Ok(())
}

async fn v30_connection() -> Result<
    (
        crate::journal::request_lifecycle::tests::helpers::Scratch,
        crate::journal::Journal,
        turso::Connection,
    ),
    HostError,
> {
    use crate::journal::request_lifecycle::tests::helpers::Scratch;

    let scratch = Scratch::new()?;
    let journal = crate::journal::Journal::open(&scratch.file()).await?;
    let connection = journal.connection().await?;
    assert_current_v31(&connection).await?;
    for (name, _) in super::super::TRIGGERS {
        connection
            .execute(&format!("DROP TRIGGER IF EXISTS {name}"), ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    connection
        .execute(
            "DROP TRIGGER IF EXISTS bound_storage_path_inode_insert_guard",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let v30_table = super::super::ATTACHMENT_PATHS.replace(
        "capacity_reservation_bound_storage_paths",
        "capacity_reservation_bound_storage_paths_v30",
    );
    connection
        .execute(&v30_table, ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO capacity_reservation_bound_storage_paths_v30 (reservation_id, role, observer, mount_target, mount_root, volume_subpath) SELECT reservation_id, role, observer, mount_target, mount_root, volume_subpath FROM capacity_reservation_bound_storage_paths",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute("DROP TABLE capacity_reservation_bound_storage_paths", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "ALTER TABLE capacity_reservation_bound_storage_paths_v30 RENAME TO capacity_reservation_bound_storage_paths",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    for (_, trigger) in super::super::TRIGGERS {
        connection
            .execute(trigger, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    connection
        .execute("PRAGMA user_version = 30", ())
        .await
        .map_err(|_| HostError::Journal)?;
    assert_eq!(
        table_sql(&connection).await?,
        super::super::normalize(super::super::ATTACHMENT_PATHS)
    );
    assert_eq!(user_version(&connection).await?, 30);
    super::super::validate_v30(&connection).await?;
    Ok((scratch, journal, connection))
}

async fn table_sql(connection: &turso::Connection) -> Result<String, HostError> {
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = 'capacity_reservation_bound_storage_paths'",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map(|sql: String| super::super::normalize(&sql))
        .map_err(|_| HostError::Journal)
}

async fn assert_current_v31(connection: &turso::Connection) -> Result<(), HostError> {
    assert_eq!(
        user_version(connection).await?,
        super::super::super::JOURNAL_VERSION
    );
    assert_eq!(
        table_sql(connection).await?,
        super::super::normalize(&super::super::attachment_paths_with_inode())
    );
    Ok(())
}

async fn user_version(connection: &turso::Connection) -> Result<i64, HostError> {
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
