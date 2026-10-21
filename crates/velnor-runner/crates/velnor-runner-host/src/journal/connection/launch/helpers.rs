#![cfg(test)]

//! Exact SQL identity comparisons shared by launch operations.

use crate::error::HostError;

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
