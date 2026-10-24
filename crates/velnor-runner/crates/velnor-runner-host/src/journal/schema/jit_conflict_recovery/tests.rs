use crate::error::HostError;
use crate::journal::{CapacityLimits, Journal, ReservationState};

#[tokio::test]
async fn v38_migration_preserves_historical_uncertain_jit_charge() -> Result<(), HostError> {
    use crate::journal::request_lifecycle::tests::helpers::{
        Scratch, binding, created_scope, provision_acquired_request_with_initialization,
    };

    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 2_000,
            memory_bytes: 25_769_803_776,
        })
        .await?;
    let repository = binding("jit-v38-preserve", 101)?;
    let scope = created_scope(&journal, &repository, 501).await?;
    let physical = provision_acquired_request_with_initialization(&scope, 72, 'd', 'd').await?;
    let runner_name = format!("v{}", "d".repeat(32));
    let (policy_generation, policy_digest) = scope.test_active_authority_identity().await?;
    let claim = scope
        .claim_runner_jit_for_request(
            physical.intent_id,
            physical.reservation_id,
            72,
            &runner_name,
            policy_generation,
            &policy_digest,
        )
        .await?
        .ok_or(HostError::Journal)?;
    scope.mark_runner_jit_uncertain(claim).await?;
    drop(scope);
    drop(journal);

    let database_path = scratch.file();
    downgrade_receipt_schema_to_v38(&database_path).await?;

    let reopened = Journal::open(&database_path).await?;
    let reopened_scope = reopened.connection_scope(&repository).await?;
    let reservations = reopened_scope.physical_runner_reservations().await?;
    let row = reservations
        .iter()
        .find(|row| row.reservation_id == physical.reservation_id)
        .ok_or(HostError::Journal)?;
    assert_eq!(row.reservation.state, ReservationState::Uncertain);
    assert_eq!(format!("{:?}", row.jit_state), "Uncertain");
    assert_eq!(
        reopened_scope
            .test_jit_conflict_receipt_count(physical.intent_id)
            .await?,
        0
    );
    Ok(())
}

async fn downgrade_receipt_schema_to_v38(database_path: &std::path::Path) -> Result<(), HostError> {
    let database = turso::Builder::new_local(database_path.to_str().ok_or(HostError::Path)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    super::remove_receipt_schema_for_migration_test(&connection).await?;
    connection
        .execute("PRAGMA user_version=38", ())
        .await
        .map_err(|_| HostError::Journal)?;
    drop(connection);
    drop(database);
    Ok(())
}

#[tokio::test]
async fn v39_schema_validation_rejects_jit_index_and_effect_fence_tampering()
-> Result<(), HostError> {
    for (index_sql, trigger_sql) in [
        (
            "DROP INDEX connection_runner_jit_witness; CREATE UNIQUE INDEX connection_runner_jit_witness ON connection_runner_jits(connection_id, scale_set_id, witness_request_id) WHERE state='uncertain'",
            None,
        ),
        (
            "DROP INDEX connection_runner_jit_witness; CREATE UNIQUE INDEX connection_runner_jit_witness ON connection_runner_jits(connection_id, scale_set_id, witness_request_id) WHERE state != 'cleaned'",
            Some((
                "DROP TRIGGER post_jit_listener_fence",
                "CREATE TRIGGER post_jit_listener_fence BEFORE UPDATE OF delivery_state ON capacity_reservation_provisioning_state WHEN 0 BEGIN SELECT RAISE(ABORT,'tampered'); END",
            )),
        ),
    ] {
        let scratch = crate::journal::request_lifecycle::tests::helpers::Scratch::new()?;
        let database_path = scratch.file();
        drop(Journal::open(&database_path).await?);
        let database = turso::Builder::new_local(database_path.to_str().ok_or(HostError::Path)?)
            .build()
            .await
            .map_err(|_| HostError::Journal)?;
        let connection = database.connect().map_err(|_| HostError::Journal)?;
        for statement in index_sql.split(';') {
            connection
                .execute(statement, ())
                .await
                .map_err(|_| HostError::Journal)?;
        }
        if let Some((drop_sql, create_sql)) = trigger_sql {
            for statement in [drop_sql, create_sql] {
                connection
                    .execute(statement, ())
                    .await
                    .map_err(|_| HostError::Journal)?;
            }
        }
        drop(connection);
        drop(database);
        assert!(Journal::open(&database_path).await.is_err());
    }
    Ok(())
}

#[tokio::test]
async fn v39_conflict_receipt_binds_acquired_digest_without_mutating_attempt()
-> Result<(), HostError> {
    use crate::journal::request_lifecycle::tests::helpers::{
        Scratch, binding, created_scope, provision_acquired_request_with_initialization,
    };

    let scratch = Scratch::new()?;
    let database_path = scratch.file();
    let journal = Journal::open(&database_path).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 2_000,
            memory_bytes: 25_769_803_776,
        })
        .await?;
    let repository = binding("jit-null-acquire-digest", 101)?;
    let scope = created_scope(&journal, &repository, 501).await?;
    let physical = provision_acquired_request_with_initialization(&scope, 73, 'e', 'e').await?;
    let runner_name = format!("v{}", "e".repeat(32));
    let (generation, digest) = scope.test_active_authority_identity().await?;
    let claim = scope
        .claim_runner_jit_for_request(
            physical.intent_id,
            physical.reservation_id,
            73,
            &runner_name,
            generation,
            &digest,
        )
        .await?
        .ok_or(HostError::Journal)?;
    scope
        .mark_runner_jit_conflict_received(claim, physical.reservation_id, generation, &digest)
        .await?;
    let (attempt_digest, auth_digest, receipt_digest) =
        conflict_receipt_digests(&scope, physical.intent_id).await?;
    assert_eq!(attempt_digest, None);
    let auth_digest = auth_digest.ok_or(HostError::Journal)?;
    assert_eq!(receipt_digest.as_deref(), Some(auth_digest.as_str()));
    drop(scope);
    drop(journal);

    drop(Journal::open(&database_path).await?);

    let different_digest = different_digest(&auth_digest);
    assert!(!set_receipt_acquire_digest(&database_path, physical.intent_id, None,).await?);
    assert!(Journal::open(&database_path).await.is_ok());

    set_attempt_acquire_digest(&database_path, physical.intent_id, Some(&different_digest)).await?;
    assert!(Journal::open(&database_path).await.is_err());
    set_attempt_acquire_digest(&database_path, physical.intent_id, None).await?;
    assert_acquired_authorization_rejects_null_digest(
        &database_path,
        repository.connection_id.as_str(),
        73,
    )
    .await?;

    assert!(
        set_receipt_acquire_digest(&database_path, physical.intent_id, Some(&different_digest),)
            .await?
    );
    assert!(Journal::open(&database_path).await.is_err());
    Ok(())
}

async fn conflict_receipt_digests(
    scope: &crate::journal::ConnectionJournal,
    intent_id: i64,
) -> Result<(Option<String>, Option<String>, Option<String>), HostError> {
    crate::journal::connection::storage::read_transaction(scope.journal_ref(), async |connection| {
        let mut rows = connection
            .query(
                "SELECT attempt.acquire_response_digest, auth.acquire_response_digest, receipt.acquire_response_digest FROM physical_attempts attempt JOIN connection_request_authorizations auth ON auth.connection_id=attempt.connection_id AND auth.scale_set_id=attempt.scale_set_id AND auth.request_id=attempt.witness_request_id LEFT JOIN physical_attempt_jit_conflict_receipts receipt ON receipt.intent_id=attempt.intent_id WHERE attempt.intent_id=?1",
                [intent_id],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let row = rows.next().await.map_err(|_| HostError::Journal)?;
        let Some(row) = row else {
            return Err(HostError::Journal);
        };
        let values = (
            row.get(0).map_err(|_| HostError::Journal)?,
            row.get(1).map_err(|_| HostError::Journal)?,
            row.get(2).map_err(|_| HostError::Journal)?,
        );
        if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
            return Err(HostError::Journal);
        }
        Ok(values)
    })
    .await
}

async fn set_attempt_acquire_digest(
    database_path: &std::path::Path,
    intent_id: i64,
    digest: Option<&str>,
) -> Result<(), HostError> {
    let database = turso::Builder::new_local(database_path.to_str().ok_or(HostError::Path)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type='trigger' AND name='physical_attempts_no_update'",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let trigger_sql: String = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    drop(rows);
    connection
        .execute("DROP TRIGGER physical_attempts_no_update", ())
        .await
        .map_err(|_| HostError::Journal)?;
    match digest {
        Some(digest) => {
            connection
                .execute(
                    "UPDATE physical_attempts SET acquire_response_digest=?1 WHERE intent_id=?2",
                    (digest, intent_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
        }
        None => {
            connection
                .execute(
                    "UPDATE physical_attempts SET acquire_response_digest=NULL WHERE intent_id=?1",
                    [intent_id],
                )
                .await
                .map_err(|_| HostError::Journal)?;
        }
    }
    connection
        .execute(&trigger_sql, ())
        .await
        .map_err(|_| HostError::Journal)?;
    drop(connection);
    drop(database);
    Ok(())
}

async fn set_receipt_acquire_digest(
    database_path: &std::path::Path,
    intent_id: i64,
    digest: Option<&str>,
) -> Result<bool, HostError> {
    let database = turso::Builder::new_local(database_path.to_str().ok_or(HostError::Path)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    let trigger_sql = read_and_drop_update_trigger(
        &connection,
        "physical_attempt_jit_conflict_receipt_no_update",
    )
    .await?;
    let update = match digest {
        Some(digest) => {
            connection
                .execute(
                    "UPDATE physical_attempt_jit_conflict_receipts SET acquire_response_digest=?1 WHERE intent_id=?2",
                    (digest, intent_id),
                )
                .await
        }
        None => {
            connection
                .execute(
                    "UPDATE physical_attempt_jit_conflict_receipts SET acquire_response_digest=NULL WHERE intent_id=?1",
                    [intent_id],
                )
                .await
        }
    };
    connection
        .execute(&trigger_sql, ())
        .await
        .map_err(|_| HostError::Journal)?;
    drop(connection);
    drop(database);
    Ok(update.is_ok())
}

async fn read_and_drop_update_trigger(
    connection: &turso::Connection,
    name: &str,
) -> Result<String, HostError> {
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type='trigger' AND name=?1",
            [name],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let trigger_sql: String = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    drop(rows);
    connection
        .execute(&format!("DROP TRIGGER {name}"), ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(trigger_sql)
}

fn different_digest(digest: &str) -> String {
    let replacement = if digest.starts_with('0') { '1' } else { '0' };
    format!("{replacement}{}", &digest[1..])
}

async fn assert_acquired_authorization_rejects_null_digest(
    database_path: &std::path::Path,
    connection_id: &str,
    request_id: i64,
) -> Result<(), HostError> {
    let database = turso::Builder::new_local(database_path.to_str().ok_or(HostError::Path)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    let result = connection
        .execute(
            "UPDATE connection_request_authorizations SET acquire_response_digest=NULL WHERE connection_id=?1 AND scale_set_id=501 AND request_id=?2 AND acquire_state='acquired'",
            (connection_id, request_id),
        )
        .await;
    assert!(result.is_err());
    drop(connection);
    drop(database);
    Ok(())
}
