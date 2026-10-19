use crate::error::HostError;

use super::guards::{
    LEGACY_PROOF_INSERT_GUARD, LEGACY_PROOF_TRANSITION_GUARD, PROOF_INSERT_GUARDS,
    PROOF_TRANSITION_GUARDS,
};

#[tokio::test]
async fn proof_guard_sets_upgrade_exact_legacy_and_reject_existing_missing_set()
-> Result<(), HostError> {
    let scratch = crate::journal::request_lifecycle::tests::helpers::Scratch::new()?;
    let initial = crate::journal::Journal::open(&scratch.file()).await?;
    drop(initial);

    let database = turso::Builder::new_local(scratch.file().to_str().ok_or(HostError::Journal)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    drop_guard_set(&connection).await?;
    connection
        .execute(LEGACY_PROOF_INSERT_GUARD, ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(LEGACY_PROOF_TRANSITION_GUARD, ())
        .await
        .map_err(|_| HostError::Journal)?;
    drop(connection);
    drop(database);

    let migrated = crate::journal::Journal::open(&scratch.file()).await?;
    let connection = migrated.connection().await?;
    super::validate_objects(&connection).await?;
    assert_guard_count(
        &connection,
        i64::try_from(PROOF_INSERT_GUARDS.len() + PROOF_TRANSITION_GUARDS.len())
            .map_err(|_| HostError::Journal)?,
    )
    .await?;
    drop(connection);
    drop(migrated);

    let database = turso::Builder::new_local(scratch.file().to_str().ok_or(HostError::Journal)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    drop_guard_set(&connection).await?;
    drop(connection);
    drop(database);

    if crate::journal::Journal::open(&scratch.file()).await.is_ok() {
        return Err(HostError::Journal);
    }
    let database = turso::Builder::new_local(scratch.file().to_str().ok_or(HostError::Journal)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    assert_guard_count(&connection, 0).await?;
    Ok(())
}

async fn drop_guard_set(connection: &turso::Connection) -> Result<(), HostError> {
    for (name, _) in PROOF_INSERT_GUARDS {
        connection
            .execute(&format!("DROP TRIGGER {name}"), ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    for (name, _) in PROOF_TRANSITION_GUARDS {
        connection
            .execute(&format!("DROP TRIGGER {name}"), ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

async fn assert_guard_count(
    connection: &turso::Connection,
    expected: i64,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'trigger' AND (name LIKE 'execution_proof_insert_%' OR name LIKE 'execution_proof_%_guard')",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let count = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    if count != expected {
        return Err(HostError::Journal);
    }
    Ok(())
}
