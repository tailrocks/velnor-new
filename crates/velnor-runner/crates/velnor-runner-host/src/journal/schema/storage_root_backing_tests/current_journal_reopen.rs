use super::super as root;
use super::assert_current_schema;
use crate::error::HostError;

#[tokio::test]
async fn current_journal_open_repairs_exact_legacy_attachment_checks_and_keeps_charge()
-> Result<(), HostError> {
    use crate::journal::request_lifecycle::tests::helpers::{
        Scratch, binding, created_scope, prepare_physical_initializer,
    };

    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = crate::journal::Journal::open(&path).await?;
    journal
        .configure_capacity(crate::journal::CapacityLimits {
            jobs: 1,
            cpu_millicores: 2_000,
            memory_bytes: 25_769_803_776,
        })
        .await?;
    let repository = binding("v27-legacy-attachment-reopen", 970)?;
    let scope = created_scope(&journal, &repository, 970).await?;
    let reservation_id = prepare_physical_initializer(&scope, 'b').await?;
    drop(scope);
    drop(journal);

    let database = turso::Builder::new_local(path.to_str().ok_or(HostError::Journal)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    for (name, _) in root::TRIGGERS {
        connection
            .execute(&format!("DROP TRIGGER IF EXISTS {name}"), ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    connection
        .execute("DROP TABLE capacity_reservation_bound_storage_paths", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute("DROP INDEX IF EXISTS capacity_bound_attachment_mount", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "DROP TABLE capacity_reservation_bound_storage_attachments",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            crate::journal::schema::provisioning::LEGACY_BOUND_STORAGE_ATTACHMENTS,
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(root::LEGACY_ATTACHMENT_PATHS, ())
        .await
        .map_err(|_| HostError::Journal)?;
    for (_, trigger) in root::TRIGGERS {
        connection
            .execute(trigger, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    drop(connection);
    drop(database);

    let reopened = crate::journal::Journal::open(&path).await?;
    let query = reopened.connection().await?;
    assert_charge(&query, reservation_id).await?;
    assert_current_schema(&query, true).await
}

async fn assert_charge(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<(), HostError> {
    let mut rows = connection
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
