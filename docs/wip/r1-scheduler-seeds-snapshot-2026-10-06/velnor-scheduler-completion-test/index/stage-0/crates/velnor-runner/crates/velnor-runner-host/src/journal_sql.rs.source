//! Transaction and row decoding helpers for the file-backed journal.

use crate::error::HostError;
use crate::journal::IntentState;
use crate::reconcile::IntentRow;
use uuid::Uuid;

pub(super) async fn commit_live(
    conn: &turso::Connection,
    kind: &str,
    subject: &str,
) -> Result<(i64, bool), HostError> {
    conn.execute("BEGIN IMMEDIATE", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let result = insert_live(conn, kind, subject).await;
    let ended = if result.is_ok() {
        conn.execute("COMMIT", ()).await
    } else {
        conn.execute("ROLLBACK", ()).await
    };
    ended.map_err(|_| HostError::Journal)?;
    result
}

async fn insert_live(
    conn: &turso::Connection,
    kind: &str,
    subject: &str,
) -> Result<(i64, bool), HostError> {
    if let Some(id) = live_id(conn, kind, subject).await? {
        return Ok((id, false));
    }
    let launch_id = (kind == "launch").then(|| Uuid::new_v4().simple().to_string());
    conn.execute(
        "INSERT INTO intents (kind, subject, state, launch_id) VALUES (?1, ?2, 'pending', ?3)",
        (kind.to_owned(), subject.to_owned(), launch_id),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    Ok((conn.last_insert_rowid(), true))
}

async fn live_id(
    conn: &turso::Connection,
    kind: &str,
    subject: &str,
) -> Result<Option<i64>, HostError> {
    let mut rows = conn
        .query(
            "SELECT id FROM intents WHERE kind = ?1 AND subject = ?2 AND state != 'failed' ORDER BY id LIMIT 1",
            (kind.to_owned(), subject.to_owned()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    row.get(0).map_err(|_| HostError::Journal).map(Some)
}

pub(super) fn intent_row(row: &turso::Row) -> Result<IntentRow, HostError> {
    let state_text: String = row.get(3).map_err(|_| HostError::Journal)?;
    Ok(IntentRow {
        id: row.get(0).map_err(|_| HostError::Journal)?,
        kind: row.get(1).map_err(|_| HostError::Journal)?,
        subject: row.get(2).map_err(|_| HostError::Journal)?,
        state: IntentState::parse(&state_text)?,
        docker_id: row.get(4).map_err(|_| HostError::Journal)?,
        dind_id: row.get(5).map_err(|_| HostError::Journal)?,
        github_runner_id: row.get(6).map_err(|_| HostError::Journal)?,
        cleanup_proven: row.get(7).map_err(|_| HostError::Journal)?,
        launch_id: row.get(8).map_err(|_| HostError::Journal)?,
        assignment_key: row.get(9).map_err(|_| HostError::Journal)?,
        seed_generation_id: row.get(10).map_err(|_| HostError::Journal)?,
        acquire_attempted: row.get(11).map_err(|_| HostError::Journal)?,
        acquire_resolved: row.get(12).map_err(|_| HostError::Journal)?,
        acquired: row.get(13).map_err(|_| HostError::Journal)?,
        jit_requested: row.get(14).map_err(|_| HostError::Journal)?,
        runner_completed: row.get(15).map_err(|_| HostError::Journal)?,
    })
}

pub(super) fn one_row(changed: u64) -> Result<(), HostError> {
    if changed == 1 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

pub(super) fn token_rejected(token: &str) -> bool {
    token.is_empty() || token.chars().any(|ch| matches!(ch, '\'' | '"'))
}
