//! Idempotent assignment journaling keyed by scale set and request ID.

use uuid::Uuid;

use crate::error::HostError;
use crate::journal::LaunchReservation;

/// Persist or replay one `runnerRequestId`, independent of queue redelivery.
pub(super) async fn commit_assignment(
    conn: &turso::Connection,
    set_id: i64,
    request_id: i64,
    message_id: i64,
    capacity: u32,
) -> Result<LaunchReservation, HostError> {
    let assignment_key = format!("{set_id}:{request_id}");
    let legacy_subject = format!("m{message_id}r{request_id}");
    conn.execute("BEGIN IMMEDIATE", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let result = assignment_id(conn, &assignment_key, &legacy_subject, request_id, capacity).await;
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
    request_id: i64,
    capacity: u32,
) -> Result<LaunchReservation, HostError> {
    if let Some(id) = completed_assignment(conn, assignment_key).await? {
        return Ok(LaunchReservation::Completed(id));
    }
    if let Some(reservation) = assignment_match(conn, assignment_key, capacity).await? {
        return Ok(reservation);
    }
    let legacy = legacy_ids(conn, request_id).await?;
    match legacy.as_slice() {
        [(id, subject, state, cleaned)] if subject == legacy_subject => {
            if state == "failed" && *cleaned {
                if occupied(conn).await? >= capacity {
                    return Ok(LaunchReservation::AtCapacity);
                }
                return insert_assignment(conn, assignment_key).await;
            }
            let changed = conn
                .execute(
                    "UPDATE intents SET assignment_key = ?1, acquire_attempted = CASE WHEN (launch_id IS NULL OR state = 'uncertain') AND acquire_attempted = 0 AND acquire_resolved = 0 AND acquired = 0 AND jit_requested = 0 THEN 1 ELSE acquire_attempted END, jit_requested = CASE WHEN (launch_id IS NULL OR state = 'uncertain') AND acquire_attempted = 0 AND acquire_resolved = 0 AND acquired = 0 AND jit_requested = 0 THEN 1 ELSE jit_requested END WHERE id = ?2 AND assignment_key IS NULL",
                    (assignment_key, *id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed == 1 {
                return assignment_match(conn, assignment_key, capacity)
                    .await?
                    .ok_or(HostError::Journal);
            }
            return Err(HostError::Journal);
        }
        [] => {}
        // The old subject omitted the scale-set id. A request seen under a
        // new message id cannot be assigned safely to another scale set.
        _ => return Err(HostError::Journal),
    }
    if occupied(conn).await? >= capacity {
        return Ok(LaunchReservation::AtCapacity);
    }
    insert_assignment(conn, assignment_key).await
}

async fn completed_assignment(
    conn: &turso::Connection,
    assignment_key: &str,
) -> Result<Option<i64>, HostError> {
    let mut rows = conn
        .query(
            "SELECT id FROM intents WHERE kind = 'launch' AND assignment_key = ?1 AND runner_completed = 1 ORDER BY id DESC LIMIT 2",
            [assignment_key],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let id: i64 = row.get(0).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(Some(id))
}

async fn legacy_ids(
    conn: &turso::Connection,
    request_id: i64,
) -> Result<Vec<(i64, String, String, bool)>, HostError> {
    let suffix = format!("r{request_id}");
    let pattern = format!("%{suffix}");
    let mut rows = conn
        .query(
            "SELECT id, subject, state, cleanup_proven FROM intents WHERE kind = 'launch' AND assignment_key IS NULL AND subject LIKE ?1 ORDER BY id LIMIT 3",
            [pattern],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut matches = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let id = row.get(0).map_err(|_| HostError::Journal)?;
        let subject: String = row.get(1).map_err(|_| HostError::Journal)?;
        let state: String = row.get(2).map_err(|_| HostError::Journal)?;
        let cleaned: bool = row.get(3).map_err(|_| HostError::Journal)?;
        if old_request(&subject) == Some(request_id) {
            matches.push((id, subject, state, cleaned));
        } else {
            // A suffix-shaped legacy row with an invalid identity is not safe
            // to ignore. It may be the same request written by an old daemon.
            return Err(HostError::Journal);
        }
    }
    Ok(matches)
}

fn old_request(subject: &str) -> Option<i64> {
    let value = subject.strip_prefix('m')?;
    let (message, request) = value.split_once('r')?;
    if message.is_empty()
        || request.is_empty()
        || !message.bytes().all(|byte| byte.is_ascii_digit())
        || !request.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    request.parse().ok()
}

async fn assignment_match(
    conn: &turso::Connection,
    assignment_key: &str,
    capacity: u32,
) -> Result<Option<LaunchReservation>, HostError> {
    let mut rows = conn
        .query(
            "SELECT id FROM intents WHERE kind = 'launch' AND assignment_key = ?1 AND cleanup_proven = 0 ORDER BY id LIMIT 2",
            [assignment_key],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let id: i64 = row.get(0).map_err(|_| HostError::Journal)?;
        if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
            return Err(HostError::Journal);
        }
        quarantine_unknown_effects(conn, id).await?;
        return Ok(Some(LaunchReservation::Existing(id)));
    }
    let Some((id, state, cleaned)) = latest_assignment(conn, assignment_key).await? else {
        return Ok(None);
    };
    if state == "done" {
        return Ok(Some(LaunchReservation::Existing(id)));
    }
    if state == "failed" && cleaned {
        if occupied(conn).await? >= capacity {
            return Ok(Some(LaunchReservation::AtCapacity));
        }
        return insert_assignment(conn, assignment_key).await.map(Some);
    }
    Ok(Some(LaunchReservation::Existing(id)))
}

async fn latest_assignment(
    conn: &turso::Connection,
    assignment_key: &str,
) -> Result<Option<(i64, String, bool)>, HostError> {
    let mut rows = conn
        .query(
            "SELECT id, state, cleanup_proven FROM intents WHERE kind = 'launch' AND assignment_key = ?1 ORDER BY id DESC LIMIT 1",
            [assignment_key],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    Ok(Some((
        row.get(0).map_err(|_| HostError::Journal)?,
        row.get(1).map_err(|_| HostError::Journal)?,
        row.get(2).map_err(|_| HostError::Journal)?,
    )))
}

async fn insert_assignment(
    conn: &turso::Connection,
    assignment_key: &str,
) -> Result<LaunchReservation, HostError> {
    let launch_id = Uuid::new_v4().simple().to_string();
    let subject = format!("s{assignment_key}");
    conn.execute(
        "INSERT INTO intents (kind, subject, state, launch_id, assignment_key) VALUES ('launch', ?1, 'pending', ?2, ?3)",
        (subject, launch_id, assignment_key),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    Ok(LaunchReservation::New(conn.last_insert_rowid()))
}

/// Begin or replay one generic scale launch under the same atomic capacity
/// check as assignment reservations.
pub(super) async fn commit_launch(
    conn: &turso::Connection,
    subject: &str,
    capacity: u32,
) -> Result<LaunchReservation, HostError> {
    conn.execute("BEGIN IMMEDIATE", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let result = launch_id(conn, subject, capacity).await;
    let ended = if result.is_ok() {
        conn.execute("COMMIT", ()).await
    } else {
        conn.execute("ROLLBACK", ()).await
    };
    ended.map_err(|_| HostError::Journal)?;
    result
}

async fn launch_id(
    conn: &turso::Connection,
    subject: &str,
    capacity: u32,
) -> Result<LaunchReservation, HostError> {
    if let Some(id) = live_launch_id(conn, subject).await? {
        return Ok(LaunchReservation::Existing(id));
    }
    if occupied(conn).await? >= capacity {
        return Ok(LaunchReservation::AtCapacity);
    }
    let launch_id = Uuid::new_v4().simple().to_string();
    conn.execute(
        "INSERT INTO intents (kind, subject, state, launch_id) VALUES ('launch', ?1, 'pending', ?2)",
        (subject, launch_id),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    Ok(LaunchReservation::New(conn.last_insert_rowid()))
}

async fn live_launch_id(conn: &turso::Connection, subject: &str) -> Result<Option<i64>, HostError> {
    let mut rows = conn
        .query(
            "SELECT id FROM intents WHERE kind = 'launch' AND subject = ?1 AND state != 'failed' ORDER BY id LIMIT 2",
            [subject],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let first = rows.next().await.map_err(|_| HostError::Journal)?;
    let Some(row) = first else {
        return Ok(None);
    };
    let id = row.get(0).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    quarantine_unknown_effects(conn, id).await?;
    Ok(Some(id))
}

async fn quarantine_unknown_effects(conn: &turso::Connection, id: i64) -> Result<(), HostError> {
    conn.execute(
        "UPDATE intents SET acquire_attempted = 1, jit_requested = 1 WHERE id = ?1 AND kind = 'launch' AND cleanup_proven = 0 AND (launch_id IS NULL OR state = 'uncertain') AND acquire_attempted = 0 AND acquire_resolved = 0 AND acquired = 0 AND jit_requested = 0",
        [id],
    )
    .await
    .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn occupied(conn: &turso::Connection) -> Result<u32, HostError> {
    let mut rows = conn
        .query(
            "SELECT count(*) FROM intents WHERE kind = 'launch' AND cleanup_proven = 0",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let count: i64 = row.get(0).map_err(|_| HostError::Journal)?;
    u32::try_from(count).map_err(|_| HostError::Journal)
}
