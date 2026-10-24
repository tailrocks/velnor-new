//! Version 8 journal with one source receipt that has no binding.

use super::LegacyAuthorization;
use crate::error::HostError;

pub(super) async fn write(
    path: &std::path::Path,
    payload: &LegacyAuthorization<'_>,
) -> Result<(), HostError> {
    let database = turso::Builder::new_local(path.to_str().ok_or(HostError::Journal)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    crate::journal::migration_tests::version_seven_fixture::install(&connection).await?;
    insert_scope_and_launch(&connection, payload).await?;
    install_v8_source_authorization(&connection, payload).await?;
    connection
        .execute("PRAGMA user_version = 8", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn insert_scope_and_launch(
    connection: &turso::Connection,
    payload: &LegacyAuthorization<'_>,
) -> Result<(), HostError> {
    connection
        .execute(
            "INSERT INTO connection_scopes (connection_id, identity) VALUES (?1, ?2)",
            (payload.connection_id, payload.identity.as_str()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO connection_scale_set_intents (connection_id, binding_digest, repository_id, repository, selector_name, selector_labels, runner_group_id, state, remote_id, remote_name, remote_labels, disable_update, ownership) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'created', ?8, ?5, ?6, 1, 'created')",
            (
                payload.connection_id,
                payload.binding_digest.as_str(),
                payload.repository_id,
                payload.repository.as_str(),
                payload.selector_name.as_str(),
                payload.selector_labels.as_str(),
                payload.runner_group_id,
                payload.scale_set_id,
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO connection_runtime_bindings (connection_id, scale_set_id, scale_set_name, set_ownership, engine_id, docker_endpoint, platform) VALUES (?1, ?2, ?3, 'created', 'test-engine', 'unix:///var/run/docker.sock', 'linux/amd64')",
            (
                payload.connection_id,
                payload.scale_set_id,
                payload.selector_name.as_str(),
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO connection_intents (id, connection_id, kind, subject, state, scale_set_id, runner_request_id, runner_name, assignment_key) VALUES (?1, ?2, 'launch', ?3, 'pending', ?4, ?5, ?6, ?7)",
            (
                payload.intent_id,
                payload.connection_id,
                format!("request:{}", payload.request_id),
                payload.scale_set_id,
                payload.request_id,
                format!("v{}", payload.request_id),
                payload.assignment_key.as_str(),
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO capacity_reservations (intent_id, connection_id, assignment_key, engine_id, docker_root_digest, binding_digest, cpu_millicores, memory_bytes, state) VALUES (?1, ?2, ?3, 'test-engine', ?4, ?5, 2000, 8589934592, 'reserved')",
            (
                payload.intent_id,
                payload.connection_id,
                payload.assignment_key.as_str(),
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                payload.binding_digest.as_str(),
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn install_v8_source_authorization(
    connection: &turso::Connection,
    payload: &LegacyAuthorization<'_>,
) -> Result<(), HostError> {
    connection
        .execute(
            "CREATE TABLE IF NOT EXISTS connection_source_authorizations (connection_id TEXT NOT NULL REFERENCES connection_scopes(connection_id), scale_set_id INTEGER NOT NULL CHECK (scale_set_id > 0), request_id INTEGER NOT NULL CHECK (request_id > 0), intent_id INTEGER NOT NULL UNIQUE REFERENCES connection_intents(id), message_id INTEGER NOT NULL CHECK (message_id >= 0), policy_mode TEXT NOT NULL CHECK (policy_mode IN ('repository', 'branch_restricted')), authority_model TEXT NOT NULL CHECK (authority_model = 'created_empty_exclusive_admin_session_v1'), binding_digest TEXT NOT NULL CHECK (length(binding_digest) = 64 AND binding_digest NOT GLOB '*[^0-9a-f]*'), source_digest TEXT NOT NULL CHECK (length(source_digest) = 64 AND source_digest NOT GLOB '*[^0-9a-f]*'), source_json TEXT NOT NULL CHECK (length(CAST(source_json AS BLOB)) BETWEEN 1 AND 32768), PRIMARY KEY (connection_id, scale_set_id, request_id))",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "CREATE INDEX IF NOT EXISTS connection_source_authorization_intent ON connection_source_authorizations(connection_id, scale_set_id, intent_id)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO connection_source_authorizations (connection_id, scale_set_id, request_id, intent_id, message_id, policy_mode, authority_model, binding_digest, source_digest, source_json) VALUES (?1, ?2, ?3, ?4, ?5, 'repository', 'created_empty_exclusive_admin_session_v1', ?6, ?7, ?8)",
            (
                payload.connection_id,
                payload.scale_set_id,
                payload.request_id,
                payload.intent_id,
                payload.message_id,
                payload.binding_digest.as_str(),
                payload.source_digest.as_str(),
                payload.source_json.as_str(),
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}
