#[path = "execution_authorization_ddl_tests/binding_reopen.rs"]
mod binding_reopen;
#[path = "execution_authorization_ddl_tests/migration.rs"]
mod migration;
#[path = "execution_authorization_ddl_tests/rollback.rs"]
mod rollback;

use crate::error::HostError;

use super::guards::{
    LEGACY_PROOF_INSERT_GUARD, LEGACY_PROOF_TRANSITION_GUARD, PROOF_INSERT_GUARDS,
    PROOF_TRANSITION_GUARDS,
};

#[tokio::test]
async fn bounded_receipt_checks_compile_in_turso() -> Result<(), HostError> {
    let scratch = crate::journal::request_lifecycle::tests::helpers::Scratch::new()?;
    let journal = crate::journal::Journal::open(&scratch.file()).await?;
    let connection = journal.connection().await?;
    let mut definition = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = 'connection_execution_proofs'",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let actual = definition
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<String>(0)
        .map_err(|_| HostError::Journal)?;
    if !actual.contains("CHECK ((challenge_file_inode IS NULL) = (challenge_file_bytes IS NULL))") {
        return Err(HostError::Journal);
    }
    let proof_result = connection
        .execute(
            "INSERT INTO connection_execution_proofs (intent_id, reservation_id, connection_id, scale_set_id, witness_request_id, binding_digest, generation, policy_digest, engine_id, docker_root_digest, runner_id, runner_name, runner_container_id, dind_container_id, instance_binding_digest, challenge, channel_digest, channel_json, challenge_state, challenge_json, challenge_digest, proof_state) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, 'not_requested', ?19, ?20, 'pending')",
            turso::params![1_i64, 1_i64, "test", 1_i64, 1_i64, "a".repeat(64), 1_i64, "b".repeat(64), "engine", "c".repeat(64), 1_i64, "runner", "d".repeat(64), "e".repeat(64), "f".repeat(64), "1".repeat(64), "g".repeat(64), "{}", "{}", "h".repeat(64)],
        )
        .await;
    if proof_result.is_ok() {
        return Err(HostError::Journal);
    }
    let decision_result = connection
        .execute(
            "INSERT INTO connection_execution_decisions (decision_file_inode) VALUES (NULL)",
            (),
        )
        .await;
    if decision_result.is_ok() {
        return Err(HostError::Journal);
    }
    Ok(())
}

#[tokio::test]
async fn decision_state_guard_rejects_same_state_update() -> Result<(), HostError> {
    let scratch = crate::journal::request_lifecycle::tests::helpers::Scratch::new()?;
    let journal = crate::journal::Journal::open(&scratch.file()).await?;
    let connection = journal.connection().await?;
    connection
        .execute(
            "CREATE TEMP TABLE _decision_state_probe (delivery_state TEXT NOT NULL)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let (_, trigger) = super::guards::DECISION_TRANSITION_GUARDS
        .iter()
        .find(|(name, _)| *name == "execution_decision_state_guard")
        .ok_or(HostError::Journal)?;
    let trigger = trigger.replace("connection_execution_decisions", "_decision_state_probe");
    connection
        .execute(&trigger, ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO _decision_state_probe (delivery_state) VALUES ('prepared')",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if connection
        .execute(
            "UPDATE _decision_state_probe SET delivery_state = 'prepared'",
            (),
        )
        .await
        .is_ok()
    {
        return Err(HostError::Journal);
    }
    connection
        .execute(
            "UPDATE _decision_state_probe SET delivery_state = 'requested'",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

#[tokio::test]
async fn v28_rebuild_preserves_published_proof_and_reopens() -> Result<(), HostError> {
    let fixture = crate::journal::request_lifecycle::tests::native_pre_challenge_fixture().await?;
    crate::journal::request_lifecycle::tests::publish_native_fixture_challenge(&fixture).await?;
    let reservation_id = fixture.initialized.reservation_id;
    let connection_id = fixture.scope.connection_id().as_str().to_owned();
    let database_path = fixture.database_path.clone();
    let connection = fixture.journal.connection().await?;

    let before = migration::proof_facts(&connection, reservation_id).await?;
    migration::recreate_pair_as_v27(&connection).await?;
    rollback::verify_failed_upgrades(&connection, reservation_id, &before).await?;
    crate::journal::schema::execution_authorization::upgrade_v28(&connection).await?;
    if migration::proof_facts(&connection, reservation_id).await? != before {
        return Err(HostError::Journal);
    }
    drop(connection);
    let crate::journal::request_lifecycle::tests::NativePreChallengeFixture {
        _scratch,
        journal,
        scope,
        initialized,
        runner,
        source,
        channel,
        ..
    } = fixture;
    drop(scope);
    drop(journal);
    drop(initialized);
    drop(runner);
    drop(source);
    drop(channel);

    let path_text = database_path.to_str().ok_or(HostError::Path)?;
    let reopened = turso::Builder::new_local(path_text)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = reopened.connect().map_err(|_| HostError::Journal)?;
    let after = migration::proof_facts(&connection, reservation_id).await?;
    if before != after || after.0 != connection_id {
        return Err(HostError::Journal);
    }
    super::validate_objects(&connection).await?;
    migration::replace_decisions_as_v27(&connection).await?;
    if super::upgrade_v28(&connection).await.is_ok()
        || migration::proof_facts(&connection, reservation_id).await? != before
    {
        return Err(HostError::Journal);
    }
    drop(connection);
    Ok(())
}
