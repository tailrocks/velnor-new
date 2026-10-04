//! Idempotent assignment journaling keyed by scale set and request ID.

use uuid::Uuid;

use crate::error::HostError;

/// Persist or replay one `runnerRequestId`, independent of queue redelivery.
pub(super) async fn commit_assignment(
    conn: &turso::Connection,
    set_id: i64,
    request_id: i64,
    message_id: i64,
) -> Result<i64, HostError> {
    let assignment_key = format!("{set_id}:{request_id}");
    let legacy_subject = format!("m{message_id}r{request_id}");
    conn.execute("BEGIN IMMEDIATE", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let result = assignment_id(conn, &assignment_key, &legacy_subject).await;
    let ended = if result.is_ok() {
        conn.execute("COMMIT", ()).await
    } else {
        conn.execute("ROLLBACK", ()).await
    };
    ended.map_err(|_| HostError::Journal)?;
    result
}

async fn assignment_id(
    conn: &turso::Connection,
    assignment_key: &str,
    legacy_subject: &str,
) -> Result<i64, HostError> {
    let matches = ids(
        conn,
        "SELECT id FROM intents WHERE kind = 'launch' AND assignment_key = ?1 ORDER BY id LIMIT 2",
        assignment_key,
    )
    .await?;
    match matches.as_slice() {
        [id] => return Ok(*id),
        [] => {}
        _ => return Err(HostError::Journal),
    }
    let legacy = ids(
        conn,
        "SELECT id FROM intents WHERE kind = 'launch' AND subject = ?1 AND assignment_key IS NULL ORDER BY id LIMIT 2",
        legacy_subject,
    )
    .await?;
    match legacy.as_slice() {
        [id] => {
            let changed = conn
                .execute(
                    "UPDATE intents SET assignment_key = ?1 WHERE id = ?2 AND assignment_key IS NULL",
                    (assignment_key, *id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed == 1 {
                return Ok(*id);
            }
            return Err(HostError::Journal);
        }
        [] => {}
        _ => return Err(HostError::Journal),
    }
    insert_assignment(conn, assignment_key).await
}

async fn ids(conn: &turso::Connection, sql: &str, key: &str) -> Result<Vec<i64>, HostError> {
    let mut rows = conn
        .query(sql, [key])
        .await
        .map_err(|_| HostError::Journal)?;
    let mut ids = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        ids.push(row.get(0).map_err(|_| HostError::Journal)?);
    }
    Ok(ids)
}

async fn insert_assignment(
    conn: &turso::Connection,
    assignment_key: &str,
) -> Result<i64, HostError> {
    let launch_id = Uuid::new_v4().simple().to_string();
    let subject = format!("s{assignment_key}");
    conn.execute(
        "INSERT INTO intents (kind, subject, state, launch_id, assignment_key) VALUES ('launch', ?1, 'pending', ?2, ?3)",
        (subject, launch_id, assignment_key),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    Ok(conn.last_insert_rowid())
}
