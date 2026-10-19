use crate::error::HostError;
use crate::journal::Journal;
use crate::journal::connection::storage::read_transaction;

use super::super::helpers::Scratch;

#[tokio::test]
async fn topology_two_cleanup_proof_passes_the_split_guards() -> Result<(), HostError> {
    let fixture = super::super::review_counterexamples::ready().await?;
    super::super::helpers::record_owned_storage_cleanup(&fixture.scope, &fixture.provisioned).await
}

#[tokio::test]
async fn version_nineteen_migration_replaces_the_legacy_cleanup_guard() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    drop(journal);

    let database = turso::Builder::new_local(path.to_str().ok_or(HostError::Path)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    for name in [
        "provisioning_storage_cleanup_identity_guard",
        "provisioning_storage_cleanup_shared_run_guard",
        "provisioning_storage_cleanup_runner_home_guard",
        "provisioning_storage_cleanup_runner_work_guard",
        "provisioning_storage_cleanup_dind_data_guard",
        "provisioning_storage_cleanup_topology_v1_guard",
        "provisioning_storage_cleanup_topology_v2_guard",
    ] {
        connection
            .execute(&format!("DROP TRIGGER {name}"), ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    connection
        .execute(
            "CREATE TRIGGER provisioning_storage_cleanup_guard BEFORE UPDATE OF storage_cleanup_proven ON capacity_reservation_provisioning_state BEGIN SELECT RAISE(ABORT, 'legacy guard'); END",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute("PRAGMA user_version = 19", ())
        .await
        .map_err(|_| HostError::Journal)?;
    drop(connection);
    drop(database);

    let migrated = Journal::open(&path).await?;
    let counts = read_transaction(&migrated, async |connection| {
        let mut rows = connection
            .query(
                "SELECT (SELECT COUNT(*) FROM sqlite_schema WHERE type = 'trigger' AND name = 'provisioning_storage_cleanup_guard'), (SELECT COUNT(*) FROM sqlite_schema WHERE type = 'trigger' AND name GLOB 'provisioning_storage_cleanup_*_guard')",
                (),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let row = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?;
        Ok((
            row.get::<i64>(0).map_err(|_| HostError::Journal)?,
            row.get::<i64>(1).map_err(|_| HostError::Journal)?,
        ))
    })
    .await?;
    assert_eq!(counts, (0, 7));
    Ok(())
}
