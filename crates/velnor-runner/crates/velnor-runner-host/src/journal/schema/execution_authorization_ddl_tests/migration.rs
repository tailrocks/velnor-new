use super::*;

#[tokio::test]
async fn synthetic_v27_pair_rebuild_preserves_published_proof() -> Result<(), HostError> {
    let fixture = crate::journal::request_lifecycle::tests::native_pre_challenge_fixture().await?;
    crate::journal::request_lifecycle::tests::publish_native_fixture_challenge(&fixture).await?;
    let reservation_id = fixture.initialized.reservation_id;
    let database_path = fixture.database_path.clone();
    let connection = fixture._journal.connection().await?;
    let before = proof_facts(&connection, reservation_id).await?;
    recreate_pair_as_v27(&connection).await?;
    crate::journal::schema::execution_authorization::upgrade_v28(&connection).await?;
    if proof_facts(&connection, reservation_id).await? != before {
        return Err(HostError::Journal);
    }
    drop(connection);
    let crate::journal::request_lifecycle::tests::NativePreChallengeFixture {
        _scratch,
        _journal,
        ..
    } = fixture;
    drop(_journal);

    let reopened = crate::journal::Journal::open(&database_path).await?;
    let connection = reopened.connection().await?;
    if proof_facts(&connection, reservation_id).await? != before {
        return Err(HostError::Journal);
    }
    super::super::validate_objects(&connection).await?;
    drop(_scratch);
    Ok(())
}

#[tokio::test]
async fn v27_journal_open_runs_historical_upgrade_dispatch() -> Result<(), HostError> {
    let scratch = crate::journal::request_lifecycle::tests::helpers::Scratch::new()?;
    let path = scratch.file();
    crate::journal::schema::historical_fixture::install(&path, 27)
        .await
        .map_err(|_| HostError::Journal)?;
    let journal = crate::journal::Journal::open(&path).await?;
    let connection = journal.connection().await?;
    if crate::journal::schema::journal_version(&connection).await?
        != crate::journal::schema::JOURNAL_VERSION
    {
        return Err(HostError::Journal);
    }
    super::super::validate_objects(&connection).await?;
    Ok(())
}

pub(super) type ProofFacts = (String, String, String, String, Option<i64>);

pub(super) async fn proof_facts(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<ProofFacts, HostError> {
    let mut rows = connection
        .query(
            "SELECT connection_id, challenge, challenge_digest, channel_digest, challenge_file_inode FROM connection_execution_proofs WHERE reservation_id = ?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let facts = (
        row.get(0).map_err(|_| HostError::Journal)?,
        row.get(1).map_err(|_| HostError::Journal)?,
        row.get(2).map_err(|_| HostError::Journal)?,
        row.get(3).map_err(|_| HostError::Journal)?,
        row.get(4).map_err(|_| HostError::Journal)?,
    );
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(facts)
}

pub(super) async fn recreate_pair_as_v27(connection: &turso::Connection) -> Result<(), HostError> {
    save_v27_pair_rows(connection).await?;
    drop_current_pair_guards(connection).await?;
    rebuild_v27_pair_tables(connection).await?;
    install_v27_pair_guards(connection).await
}

async fn save_v27_pair_rows(connection: &turso::Connection) -> Result<(), HostError> {
    for query in [
        "CREATE TEMP TABLE _proof_rows AS SELECT * FROM connection_execution_proofs",
        "CREATE TEMP TABLE _decision_rows AS SELECT * FROM connection_execution_decisions",
    ] {
        connection
            .execute(query, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

async fn drop_current_pair_guards(connection: &turso::Connection) -> Result<(), HostError> {
    for (name, _) in PROOF_INSERT_GUARDS.iter().chain(PROOF_TRANSITION_GUARDS) {
        drop_trigger(connection, name).await?;
    }
    for (name, _) in super::super::guards::DECISION_TRANSITION_GUARDS {
        drop_trigger(connection, name).await?;
    }
    for (kind, name, _) in super::super::guards::OBJECTS {
        if *kind == "trigger"
            && (name.contains("proof")
                || name.contains("decision")
                || *name == "execution_listener_challenge_guard"
                || *name == "execution_reservation_release_guard")
        {
            drop_trigger(connection, name).await?;
        }
    }
    for prefix in ["execution_proofs", "execution_decisions"] {
        for action in ["insert", "update", "delete"] {
            drop_trigger(connection, &format!("{prefix}_revision_{action}")).await?;
        }
    }
    Ok(())
}

async fn rebuild_v27_pair_tables(connection: &turso::Connection) -> Result<(), HostError> {
    connection
        .execute("DROP INDEX execution_actual_request", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute("DROP TABLE connection_execution_decisions", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute("DROP TABLE connection_execution_proofs", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(super::super::legacy_v27::PROOF_TABLE, ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO connection_execution_proofs SELECT * FROM _proof_rows",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(super::super::legacy_v27::DECISION_TABLE, ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO connection_execution_decisions SELECT * FROM _decision_rows",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute("DROP TABLE _decision_rows", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute("DROP TABLE _proof_rows", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn install_v27_pair_guards(connection: &turso::Connection) -> Result<(), HostError> {
    for (_, statement) in PROOF_INSERT_GUARDS
        .iter()
        .chain(super::super::guards::V27_PROOF_TRANSITION_GUARDS)
    {
        connection
            .execute(statement, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    for (_, _, statement) in super::super::guards::OBJECTS {
        connection
            .execute(statement, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    for statement in [
        super::super::guards::LEGACY_DECISION_TRANSITION_GUARD,
        super::super::guards::ACTUAL_REQUEST_INDEX,
    ] {
        connection
            .execute(statement, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    super::super::install_revision_triggers(connection).await
}

pub(super) async fn user_version(connection: &turso::Connection) -> Result<i64, HostError> {
    let mut version = connection
        .query("PRAGMA user_version", ())
        .await
        .map_err(|_| HostError::Journal)?;
    version
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)
}

pub(super) async fn replace_decisions_as_v27(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    connection
        .execute(
            "CREATE TEMP TABLE _decision_rows AS SELECT * FROM connection_execution_decisions",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    for (kind, name, _) in super::super::guards::OBJECTS {
        if *kind == "trigger" && name.contains("decision") {
            drop_trigger(connection, name).await?;
        }
    }
    for (name, _) in super::super::guards::DECISION_TRANSITION_GUARDS {
        drop_trigger(connection, name).await?;
    }
    connection
        .execute("DROP TABLE connection_execution_decisions", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(super::super::legacy_v27::DECISION_TABLE, ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(super::super::guards::LEGACY_DECISION_TRANSITION_GUARD, ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO connection_execution_decisions SELECT * FROM _decision_rows",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute("DROP TABLE _decision_rows", ())
        .await
        .map(|_| ())
        .map_err(|_| HostError::Journal)
}

async fn drop_trigger(connection: &turso::Connection, name: &str) -> Result<(), HostError> {
    connection
        .execute(&format!("DROP TRIGGER IF EXISTS {name}"), ())
        .await
        .map(|_| ())
        .map_err(|_| HostError::Journal)
}

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
    super::super::validate_objects(&connection).await?;
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
