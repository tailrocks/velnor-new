//! Exact SQL identity comparisons shared by launch operations.

use crate::error::HostError;

pub(super) async fn bind_ids(
    connection: &turso::Connection,
    id: i64,
    connection_id: &str,
    docker_id: Option<&str>,
    github_runner_id: Option<&str>,
    dind_id: Option<&str>,
) -> Result<(), HostError> {
    let changed = connection
        .execute(
            "UPDATE connection_intents SET docker_id = COALESCE(docker_id, ?1), github_runner_id = COALESCE(github_runner_id, ?2), dind_id = COALESCE(dind_id, ?3) WHERE id = ?4 AND connection_id = ?5 AND kind = 'launch' AND cleanup_proven = 0 AND (?1 IS NULL OR docker_id IS NULL OR docker_id = ?1) AND (?2 IS NULL OR github_runner_id IS NULL OR github_runner_id = ?2) AND (?3 IS NULL OR dind_id IS NULL OR dind_id = ?3)",
            (docker_id, github_runner_id, dind_id, id, connection_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed == 1 {
        return Ok(());
    }
    let mut rows = connection
        .query(
            "SELECT docker_id, github_runner_id, dind_id FROM connection_intents WHERE id = ?1 AND connection_id = ?2",
            (id, connection_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let actual_docker: Option<String> = row.get(0).map_err(|_| HostError::Journal)?;
    let actual_runner: Option<String> = row.get(1).map_err(|_| HostError::Journal)?;
    let actual_dind: Option<String> = row.get(2).map_err(|_| HostError::Journal)?;
    if docker_id.is_none_or(|want| actual_docker.as_deref() == Some(want))
        && github_runner_id.is_none_or(|want| actual_runner.as_deref() == Some(want))
        && dind_id.is_none_or(|want| actual_dind.as_deref() == Some(want))
    {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

pub(super) async fn same_optional(
    connection: &turso::Connection,
    id: i64,
    connection_id: &str,
    column: &str,
    value: &str,
) -> Result<bool, HostError> {
    let query =
        format!("SELECT {column} FROM connection_intents WHERE id = ?1 AND connection_id = ?2");
    let mut rows = connection
        .query(&query, (id, connection_id))
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(false);
    };
    let stored: Option<String> = row.get(0).map_err(|_| HostError::Journal)?;
    Ok(stored.as_deref() == Some(value))
}

pub(super) async fn same_runner_identity(
    connection: &turso::Connection,
    id: i64,
    connection_id: &str,
    request_id: i64,
    runner_name: &str,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT runner_request_id, runner_name FROM connection_intents WHERE id = ?1 AND connection_id = ?2",
            (id, connection_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(false);
    };
    Ok(
        row.get::<Option<i64>>(0).map_err(|_| HostError::Journal)? == Some(request_id)
            && row
                .get::<Option<String>>(1)
                .map_err(|_| HostError::Journal)?
                .as_deref()
                == Some(runner_name),
    )
}
