use crate::config::{ApprovedJobAuthorityPolicy, HostConfig, RepositoryConnection};
use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::connection::storage::in_transaction;

use super::{policy_digest, records::digest};

const MAX_POLICY_BYTES: usize = 256 * 1024;

struct PolicyInstall {
    connection_id: String,
    identity: String,
    binding_digest: String,
    generation: i64,
    policy_digest: String,
    full_json: String,
}

pub(super) async fn install(
    scope: &ConnectionJournal,
    config: &HostConfig,
    policy: &ApprovedJobAuthorityPolicy,
) -> Result<(), HostError> {
    let binding = config
        .connection(scope.connection_id())
        .ok_or(HostError::Identity)?;
    let full_json = serde_json::to_vec(policy).map_err(|_| HostError::Journal)?;
    let content_digest = policy_digest(policy)?;
    let generation = i64::try_from(policy.generation()).map_err(|_| HostError::Journal)?;
    if full_json.len() > MAX_POLICY_BYTES
        || generation <= 0
        || policy.connection_id() != scope.connection_id()
        || policy.binding_sha256() != scope.binding_digest()
        || policy.policy_sha256() != content_digest
    {
        return Err(HostError::Identity);
    }
    policy
        .validate_for(binding)
        .map_err(|_| HostError::Identity)?;
    let connection_id = scope.connection_id().as_str().to_owned();
    let identity = identity_json(binding)?;
    let full_json = String::from_utf8(full_json).map_err(|_| HostError::Journal)?;
    let install = PolicyInstall {
        connection_id,
        identity,
        binding_digest: policy.binding_sha256().to_owned(),
        generation,
        policy_digest: policy.policy_sha256().to_owned(),
        full_json,
    };
    in_transaction(scope.journal_ref(), async |connection| {
        scope.validate_scope(connection).await?;
        install_in_transaction(connection, &install).await
    })
    .await
}

async fn install_in_transaction(
    connection: &turso::Connection,
    install: &PolicyInstall,
) -> Result<(), HostError> {
    ensure_connection_identity(
        connection,
        &install.connection_id,
        &install.identity,
        &install.binding_digest,
    )
    .await?;
    let current = active_policy(connection, &install.connection_id).await?;
    if current
        .as_ref()
        .is_some_and(|(active_generation, active_digest)| {
            *active_generation == install.generation && active_digest == &install.policy_digest
        })
    {
        return ensure_snapshot(connection, install).await;
    }
    if current
        .as_ref()
        .is_some_and(|(active_generation, _)| install.generation <= *active_generation)
    {
        return Err(HostError::Identity);
    }
    if current.is_none() && policy_history_exists(connection, &install.connection_id).await? {
        return Err(HostError::Journal);
    }
    if current.is_some()
        && unresolved_decision_delivery_exists(connection, &install.connection_id).await?
    {
        return Err(HostError::Journal);
    }

    ensure_snapshot(connection, install).await?;
    match current {
        Some(_) => update_active_policy(connection, install).await,
        None => insert_active_policy(connection, install).await,
    }
}

async fn update_active_policy(
    connection: &turso::Connection,
    install: &PolicyInstall,
) -> Result<(), HostError> {
    let changed = connection
        .execute(
            "UPDATE connection_active_job_authority SET generation = ?2, policy_digest = ?3 WHERE connection_id = ?1",
            (
                install.connection_id.as_str(),
                install.generation,
                install.policy_digest.as_str(),
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed == 1 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

async fn insert_active_policy(
    connection: &turso::Connection,
    install: &PolicyInstall,
) -> Result<(), HostError> {
    connection
        .execute(
            "INSERT INTO connection_active_job_authority (connection_id, generation, policy_digest) VALUES (?1, ?2, ?3)",
            (
                install.connection_id.as_str(),
                install.generation,
                install.policy_digest.as_str(),
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn ensure_connection_identity(
    connection: &turso::Connection,
    connection_id: &str,
    expected_identity: &str,
    expected_digest: &str,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT identity FROM connection_scopes WHERE connection_id = ?1 AND retired = 0",
            [connection_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows.next().await.map_err(|_| HostError::Journal)?;
    let Some(row) = row else {
        return Err(HostError::Journal);
    };
    let identity: String = row.get(0).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        || identity != expected_identity
        || digest(identity.as_bytes()) != expected_digest
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

async fn active_policy(
    connection: &turso::Connection,
    connection_id: &str,
) -> Result<Option<(i64, String)>, HostError> {
    let mut rows = connection
        .query(
            "SELECT generation, policy_digest FROM connection_active_job_authority WHERE connection_id = ?1",
            [connection_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let result = (
        row.get(0).map_err(|_| HostError::Journal)?,
        row.get(1).map_err(|_| HostError::Journal)?,
    );
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(Some(result))
}

async fn policy_history_exists(
    connection: &turso::Connection,
    connection_id: &str,
) -> Result<bool, HostError> {
    exists(
        connection,
        "SELECT EXISTS (SELECT 1 FROM connection_job_authority_policies WHERE connection_id = ?1)",
        connection_id,
    )
    .await
}

async fn unresolved_decision_delivery_exists(
    connection: &turso::Connection,
    connection_id: &str,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT EXISTS (SELECT 1 FROM connection_execution_decisions WHERE connection_id = ?1 AND delivery_state IN ('prepared', 'requested', 'uncertain'))",
            [connection_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let value = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    Ok(value != 0)
}

async fn ensure_snapshot(
    connection: &turso::Connection,
    install: &PolicyInstall,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT binding_digest, policy_json FROM connection_job_authority_policies WHERE connection_id = ?1 AND generation = ?2 AND policy_digest = ?3",
            (
                install.connection_id.as_str(),
                install.generation,
                install.policy_digest.as_str(),
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let stored_binding: String = row.get(0).map_err(|_| HostError::Journal)?;
        let stored_json: String = row.get(1).map_err(|_| HostError::Journal)?;
        if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
            || stored_binding != install.binding_digest
            || stored_json != install.full_json
        {
            return Err(HostError::Journal);
        }
        return Ok(());
    }
    connection
        .execute(
            "INSERT INTO connection_job_authority_policies (connection_id, generation, binding_digest, policy_digest, policy_json) VALUES (?1, ?2, ?3, ?4, ?5)",
            (
                install.connection_id.as_str(),
                install.generation,
                install.binding_digest.as_str(),
                install.policy_digest.as_str(),
                install.full_json.as_str(),
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn exists(
    connection: &turso::Connection,
    query: &str,
    connection_id: &str,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(query, [connection_id])
        .await
        .map_err(|_| HostError::Journal)?;
    let value = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    Ok(value != 0)
}

fn identity_json(binding: &RepositoryConnection) -> Result<String, HostError> {
    String::from_utf8(
        binding
            .canonical_identity_bytes()
            .map_err(|_| HostError::Identity)?,
    )
    .map_err(|_| HostError::Journal)
}
