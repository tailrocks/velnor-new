use super::{LEGACY_TRANSITION_TRIGGER, TRANSITION_TRIGGERS, bootstrap, validate};
use crate::error::HostError;

#[tokio::test]
async fn split_handoff_guards_preserve_identity_and_repair_exact_draft_trigger()
-> Result<(), HostError> {
    let path = std::env::temp_dir().join(format!(
        "velnor-handoff-trigger-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| HostError::Journal)?
            .as_nanos()
    ));
    let path_text = path.to_str().ok_or(HostError::Journal)?;
    let database = turso::Builder::new_local(path_text)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    create_prerequisites(&connection).await?;
    bootstrap(&connection).await?;
    insert_ready_held(&connection).await?;

    assert_attached_transition_guards(&connection).await?;
    repair_legacy_draft_trigger(&connection).await?;
    drop(connection);
    drop(database);

    reopen_current_version_path().await?;
    Ok(())
}

async fn assert_attached_transition_guards(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    assert_rejected(
        connection,
        "UPDATE capacity_reservation_storage_handoffs SET connection_id = 'other' WHERE reservation_id = 1",
    )
    .await?;
    assert_rejected(
        connection,
        &format!(
            "UPDATE capacity_reservation_storage_handoffs SET state = 'attached', mount_evidence_digest = '{}', runner_container_id = '{}', dind_container_id = '{}' WHERE reservation_id = 1",
            "f".repeat(64),
            "0".repeat(64),
            "e".repeat(64),
        ),
    )
    .await?;
    assert_rejected(
        connection,
        &format!(
            "UPDATE capacity_reservation_storage_handoffs SET state = 'attached', mount_evidence_digest = '{}', runner_container_id = '{}', dind_container_id = '{}' WHERE reservation_id = 1",
            "f".repeat(64),
            "d".repeat(64),
            "0".repeat(64),
        ),
    )
    .await?;

    connection
        .execute(
            &format!(
                "UPDATE capacity_reservation_storage_handoffs SET state = 'attached', mount_evidence_digest = '{}', runner_container_id = '{}', dind_container_id = '{}' WHERE reservation_id = 1",
                "f".repeat(64),
                "d".repeat(64),
                "e".repeat(64),
            ),
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn repair_legacy_draft_trigger(connection: &turso::Connection) -> Result<(), HostError> {
    // Simulate the exact unreleased v27 draft. Bootstrap may replace this
    // trigger, but must reject unknown or partial guard sets.
    for (name, _) in TRANSITION_TRIGGERS {
        connection
            .execute(&format!("DROP TRIGGER {name}"), ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    connection
        .execute(LEGACY_TRANSITION_TRIGGER, ())
        .await
        .map_err(|_| HostError::Journal)?;
    bootstrap(connection).await?;
    validate(connection).await?;

    let mut rows = connection
        .query(
            "SELECT state, runner_container_id, dind_container_id FROM capacity_reservation_storage_handoffs WHERE reservation_id = 1",
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
        row.get::<String>(0).map_err(|_| HostError::Journal)?,
        "attached"
    );
    assert_eq!(
        row.get::<String>(1).map_err(|_| HostError::Journal)?,
        "d".repeat(64)
    );
    assert_eq!(
        row.get::<String>(2).map_err(|_| HostError::Journal)?,
        "e".repeat(64)
    );
    assert_rejected(
        connection,
        "UPDATE capacity_reservation_storage_handoffs SET ready_digest = '0' WHERE reservation_id = 1",
    )
    .await?;
    Ok(())
}

async fn reopen_current_version_path() -> Result<(), HostError> {
    // Exercise the real Journal::open current-version path after restoring an
    // exact old-draft trigger. Its outer startup transaction applies the same
    // recognized-only replacement before strict schema validation.
    let scratch = crate::journal::request_lifecycle::tests::helpers::Scratch::new()?;
    let journal = crate::journal::Journal::open(&scratch.file()).await?;
    drop(journal);
    let raw_database =
        turso::Builder::new_local(scratch.file().to_str().ok_or(HostError::Journal)?)
            .build()
            .await
            .map_err(|_| HostError::Journal)?;
    let raw = raw_database.connect().map_err(|_| HostError::Journal)?;
    for (name, _) in TRANSITION_TRIGGERS {
        raw.execute(&format!("DROP TRIGGER {name}"), ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    raw.execute(LEGACY_TRANSITION_TRIGGER, ())
        .await
        .map_err(|_| HostError::Journal)?;
    drop(raw);
    drop(raw_database);
    let reopened = crate::journal::Journal::open(&scratch.file()).await?;
    let reopened_connection = reopened.connection().await?;
    validate(&reopened_connection).await?;
    drop(reopened_connection);
    drop(reopened);
    Ok(())
}

async fn create_prerequisites(connection: &turso::Connection) -> Result<(), HostError> {
    for sql in [
        "CREATE TABLE capacity_reservation_storage_initialization (reservation_id INTEGER PRIMARY KEY, connection_id TEXT, operation_id INTEGER, generation INTEGER, create_state TEXT, start_state TEXT, result_state TEXT, remove_state TEXT, layout_version INTEGER, helper_name TEXT, helper_id TEXT, engine_id TEXT, docker_root_digest TEXT, metadata_volume TEXT, metadata_volume_id TEXT, operation_key TEXT, metadata_layout_digest TEXT)",
        "CREATE TABLE capacity_reservations (id INTEGER, intent_id INTEGER, connection_id TEXT, state TEXT)",
        "CREATE TABLE capacity_reservation_provisioning_objects (reservation_id INTEGER, role TEXT, create_state TEXT, docker_name TEXT, docker_id TEXT)",
        "CREATE TABLE capacity_reservation_bootstrap_containers (reservation_id INTEGER, role TEXT, create_state TEXT, start_state TEXT, container_id TEXT)",
    ] {
        connection
            .execute(sql, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

async fn insert_ready_held(connection: &turso::Connection) -> Result<(), HostError> {
    let helper_id = "a".repeat(64);
    let layout_digest = "b".repeat(64);
    let ready_digest = "c".repeat(64);
    let runner_id = "d".repeat(64);
    let dind_id = "e".repeat(64);
    let docker_root_digest = "1".repeat(64);
    let metadata_volume_id = "3".repeat(64);
    connection
        .execute(
            "INSERT INTO capacity_reservation_storage_initialization VALUES (1,'conn',2,1,'created','started','confirmed','not_requested',2,'helper',?1,'engine',?2,'metadata-name',?3,'operation-key',?4)",
            (
                helper_id.as_str(),
                docker_root_digest.as_str(),
                metadata_volume_id.as_str(),
                layout_digest.as_str(),
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO capacity_reservations VALUES (1,2,'conn','reserved')",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO capacity_reservation_provisioning_objects VALUES (1,'engine-metadata','created','metadata-name',?1)",
            [metadata_volume_id.as_str()],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    for (role, id) in [("runner", runner_id.as_str()), ("dind", dind_id.as_str())] {
        connection
            .execute(
                "INSERT INTO capacity_reservation_bootstrap_containers VALUES (1,?1,'created','started',?2)",
                (role, id),
            )
            .await
            .map_err(|_| HostError::Journal)?;
    }
    connection
        .execute(
            "INSERT INTO capacity_reservation_storage_handoffs (reservation_id, connection_id, operation_id, generation, helper_name, helper_id, engine_id, docker_root_digest, metadata_volume_name, metadata_volume_id, operation_key, layout_digest, ready_digest, state) VALUES (1,'conn',2,1,'helper',?1,'engine',?2,'metadata-name',?3,'operation-key',?4,?5,'ready_held')",
            (
                helper_id.as_str(),
                docker_root_digest.as_str(),
                metadata_volume_id.as_str(),
                layout_digest.as_str(),
                ready_digest.as_str(),
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn assert_rejected(connection: &turso::Connection, sql: &str) -> Result<(), HostError> {
    if connection.execute(sql, ()).await.is_ok() {
        return Err(HostError::Journal);
    }
    Ok(())
}
