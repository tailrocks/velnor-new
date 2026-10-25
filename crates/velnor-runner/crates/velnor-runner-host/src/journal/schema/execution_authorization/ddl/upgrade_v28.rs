use super::{DECISION_TABLE, PROOF_TABLE, guards, legacy_v27, normalize, object_sql};
use crate::error::HostError;

const PROOF_ROWS: &str = "_execution_proof_rows_v28";
const DECISION_ROWS: &str = "_execution_decision_rows_v28";

pub(super) async fn upgrade(connection: &turso::Connection) -> Result<(), HostError> {
    upgrade_inner(connection, None).await
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum UpgradeFailpoint {
    AfterDecisionDrop,
    AfterProofCopy,
}

#[cfg(test)]
pub(super) async fn upgrade_with_test_failure(
    connection: &turso::Connection,
    failpoint: UpgradeFailpoint,
) -> Result<(), HostError> {
    upgrade_inner(connection, Some(failpoint)).await
}

async fn upgrade_inner(
    connection: &turso::Connection,
    failpoint: Option<UpgradeFailpoint>,
) -> Result<(), HostError> {
    let proof = object_sql(connection, "table", "connection_execution_proofs").await?;
    let decision = object_sql(connection, "table", "connection_execution_decisions").await?;
    let is_new_pair = normalize(&proof) == normalize(PROOF_TABLE)
        && normalize(&decision) == normalize(DECISION_TABLE);
    if is_new_pair {
        return super::validate_objects(connection).await;
    }

    let is_v27_pair = normalize(&proof) == normalize(legacy_v27::PROOF_TABLE)
        && normalize(&decision) == normalize(legacy_v27::DECISION_TABLE);
    if !is_v27_pair {
        return Err(HostError::Journal);
    }
    guards::bootstrap_decision_transition_guards(connection, true).await?;
    // The host must change the proof guards before it changes the tables.
    // Journals at version 26 or 27 require this order.
    guards::bootstrap_proof_transition_guards(connection, true).await?;
    super::validate_v27_objects(connection).await?;

    let proof_count = row_count(connection, "connection_execution_proofs").await?;
    let decision_count = row_count(connection, "connection_execution_decisions").await?;
    rebuild_tables(connection, failpoint).await?;
    restore_guards(connection).await?;

    if row_count(connection, "connection_execution_proofs").await? != proof_count
        || row_count(connection, "connection_execution_decisions").await? != decision_count
    {
        return Err(HostError::Journal);
    }
    ensure_foreign_keys(connection).await?;
    super::validate_objects(connection).await
}

async fn rebuild_tables(
    connection: &turso::Connection,
    failpoint: Option<UpgradeFailpoint>,
) -> Result<(), HostError> {
    connection
        .execute(
            &format!("CREATE TEMP TABLE {PROOF_ROWS} AS SELECT * FROM connection_execution_proofs"),
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            &format!(
                "CREATE TEMP TABLE {DECISION_ROWS} AS SELECT * FROM connection_execution_decisions"
            ),
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;

    drop_dependent_guards(connection).await?;
    connection
        .execute("DROP INDEX execution_actual_request", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute("DROP TABLE connection_execution_decisions", ())
        .await
        .map_err(|_| HostError::Journal)?;
    if failpoint == Some(UpgradeFailpoint::AfterDecisionDrop) {
        return Err(HostError::Journal);
    }
    connection
        .execute("DROP TABLE connection_execution_proofs", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(PROOF_TABLE, ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            &format!("INSERT INTO connection_execution_proofs SELECT * FROM {PROOF_ROWS}"),
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if failpoint == Some(UpgradeFailpoint::AfterProofCopy) {
        return Err(HostError::Journal);
    }
    connection
        .execute(DECISION_TABLE, ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            &format!("INSERT INTO connection_execution_decisions SELECT * FROM {DECISION_ROWS}"),
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(&format!("DROP TABLE {DECISION_ROWS}"), ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(&format!("DROP TABLE {PROOF_ROWS}"), ())
        .await
        .map_err(|_| HostError::Journal)?;

    Ok(())
}

async fn restore_guards(connection: &turso::Connection) -> Result<(), HostError> {
    for (_, statement) in guards::PROOF_INSERT_GUARDS {
        connection
            .execute(statement, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    for (_, statement) in guards::PROOF_TRANSITION_GUARDS {
        connection
            .execute(statement, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    for (_, statement) in guards::DECISION_TRANSITION_GUARDS {
        connection
            .execute(statement, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    for (_, _, statement) in guards::OBJECTS {
        connection
            .execute(statement, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    super::install_revision_triggers(connection).await?;
    Ok(())
}

async fn drop_dependent_guards(connection: &turso::Connection) -> Result<(), HostError> {
    for (name, _) in guards::PROOF_INSERT_GUARDS
        .iter()
        .chain(guards::PROOF_TRANSITION_GUARDS.iter())
        .chain(guards::DECISION_TRANSITION_GUARDS.iter())
    {
        drop_trigger(connection, name).await?;
    }
    for (kind, name, _) in guards::OBJECTS {
        if *kind == "trigger"
            && (name.contains("proof")
                || name.contains("decision")
                || *name == "execution_listener_challenge_guard"
                || *name == "execution_reservation_release_guard")
        {
            drop_trigger(connection, name).await?;
        }
    }
    for table in ["proofs", "decisions"] {
        for action in ["insert", "update", "delete"] {
            drop_trigger(connection, &format!("execution_{table}_revision_{action}")).await?;
        }
    }
    Ok(())
}

async fn drop_trigger(connection: &turso::Connection, name: &str) -> Result<(), HostError> {
    connection
        .execute(&format!("DROP TRIGGER IF EXISTS {name}"), ())
        .await
        .map(|_| ())
        .map_err(|_| HostError::Journal)
}

async fn row_count(connection: &turso::Connection, table: &str) -> Result<i64, HostError> {
    let mut rows = connection
        .query(&format!("SELECT COUNT(*) FROM {table}"), ())
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)
}

async fn ensure_foreign_keys(connection: &turso::Connection) -> Result<(), HostError> {
    let mut rows = connection
        .query("PRAGMA foreign_key_check", ())
        .await
        .map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        Err(HostError::Journal)
    } else {
        Ok(())
    }
}
