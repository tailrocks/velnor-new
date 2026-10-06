//! Assigned request reservation and one-shot acquire/JIT transitions.

use crate::error::HostError;
use crate::journal::{IntentState, Journal, one_row, token_rejected};

use super::{assigned_request, finish_transaction, is_message_name, is_session_name};

impl Journal {
    /// Persist launch identity before an external effect. Repeated identical binds are safe.
    pub(crate) async fn bind_launch_identity(
        &self,
        id: i64,
        scale_set_id: i64,
        request_id: Option<i64>,
        runner_name: &str,
    ) -> Result<(), HostError> {
        if id <= 0 || scale_set_id <= 0 || request_id.is_some_and(|value| value <= 0) {
            return Err(HostError::Journal);
        }
        if token_rejected(runner_name) {
            return Err(HostError::Journal);
        }
        let connection = self.connection().await?;
        connection
            .execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result =
            bind_identity_row(&connection, id, scale_set_id, request_id, runner_name).await;
        finish_transaction(&connection, result).await
    }

    /// Atomically reserve one assigned request before acquire or JIT.
    ///
    /// A redelivery with another message id returns the existing intent as not fresh.
    pub(crate) async fn begin_assigned_launch(
        &self,
        subject: &str,
        scale_set_id: i64,
        request_id: i64,
        runner_name: &str,
    ) -> Result<(i64, bool), HostError> {
        if scale_set_id <= 0
            || request_id <= 0
            || token_rejected(subject)
            || token_rejected(runner_name)
            || assigned_request(subject) != Some(request_id)
            || runner_name != format!("v{request_id}")
        {
            return Err(HostError::Journal);
        }
        let connection = self.connection().await?;
        connection
            .execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result =
            assigned_launch_row(&connection, subject, scale_set_id, request_id, runner_name).await;
        finish_transaction(&connection, result).await
    }

    /// Persist the acquire attempt before calling the Scale Set endpoint.
    pub(crate) async fn claim_assigned_acquire(&self, id: i64) -> Result<bool, HostError> {
        if id <= 0 {
            return Err(HostError::Journal);
        }
        let connection = self.connection().await?;
        connection
            .execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = connection
            .execute(
                "UPDATE intents SET acquire_attempted = 1 WHERE id = ?1 AND kind = 'launch' AND state = 'pending' AND cleanup_proven = 0 AND scale_set_id IS NOT NULL AND runner_request_id IS NOT NULL AND runner_name IS NOT NULL AND acquire_attempted = 0 AND acquire_resolved = 0 AND acquired = 0 AND jit_requested = 0",
                [id],
            )
            .await
            .map(|changed| changed == 1)
            .map_err(|_| HostError::Journal);
        finish_transaction(&connection, result).await
    }

    /// Record a definite `AcquireJobs` response before a JIT call.
    ///
    /// A rejected response clears the attempt marker for a safe retry; an acquired
    /// response permanently fences `AcquireJobs` for this request.
    pub(crate) async fn record_assigned_acquire(
        &self,
        id: i64,
        acquired: bool,
    ) -> Result<(), HostError> {
        if id <= 0 {
            return Err(HostError::Journal);
        }
        let connection = self.connection().await?;
        connection
            .execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let statement = if acquired {
            "UPDATE intents SET acquire_resolved = 1, acquired = 1 WHERE id = ?1 AND kind = 'launch' AND state = 'pending' AND acquire_attempted = 1 AND acquire_resolved = 0 AND acquired = 0 AND jit_requested = 0"
        } else {
            "UPDATE intents SET state = 'failed', acquire_attempted = 0, acquire_resolved = 0, acquired = 0 WHERE id = ?1 AND kind = 'launch' AND state = 'pending' AND acquire_attempted = 1 AND acquire_resolved = 0 AND acquired = 0 AND jit_requested = 0"
        };
        let result = connection
            .execute(statement, [id])
            .await
            .map_err(|_| HostError::Journal)
            .and_then(one_row);
        finish_transaction(&connection, result).await
    }

    /// Persist one JIT attempt before making its HTTP request.
    pub(crate) async fn claim_launch_jit(&self, id: i64) -> Result<bool, HostError> {
        if id <= 0 {
            return Err(HostError::Journal);
        }
        let connection = self.connection().await?;
        connection
            .execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = connection
            .execute(
                "UPDATE intents SET jit_requested = 1 WHERE id = ?1 AND kind = 'launch' AND state = 'pending' AND cleanup_proven = 0 AND runner_name IS NOT NULL AND jit_requested = 0 AND ((runner_request_id IS NULL AND acquire_attempted = 0 AND acquire_resolved = 0 AND acquired = 0) OR (runner_request_id IS NOT NULL AND acquire_attempted = 1 AND acquire_resolved = 1 AND acquired = 1))",
                [id],
            )
            .await
            .map(|changed| changed == 1)
            .map_err(|_| HostError::Journal);
        finish_transaction(&connection, result).await
    }
}

async fn bind_identity_row(
    connection: &turso::Connection,
    id: i64,
    scale_set_id: i64,
    request_id: Option<i64>,
    runner_name: &str,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT subject, scale_set_id, runner_request_id, runner_name, cleanup_proven, state FROM intents WHERE id = ?1 AND kind = 'launch'",
            [id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Err(HostError::Journal);
    };
    let subject: String = row.get(0).map_err(|_| HostError::Journal)?;
    let stored_set: Option<i64> = row.get(1).map_err(|_| HostError::Journal)?;
    let stored_request: Option<i64> = row.get(2).map_err(|_| HostError::Journal)?;
    let stored_name: Option<String> = row.get(3).map_err(|_| HostError::Journal)?;
    let cleanup_proven: bool = row.get(4).map_err(|_| HostError::Journal)?;
    let state_text: String = row.get(5).map_err(|_| HostError::Journal)?;
    let state = IntentState::parse(&state_text)?;
    if !valid_launch_identity(&subject, request_id, runner_name) {
        return Err(HostError::Journal);
    }
    let exact = stored_set == Some(scale_set_id)
        && stored_request == request_id
        && stored_name.as_deref() == Some(runner_name);
    if exact && !cleanup_proven && state != IntentState::Failed {
        return Ok(());
    }
    let legacy_done_backfill = state == IntentState::Done
        && request_id.is_some()
        && assigned_request(&subject) == request_id
        && stored_set.is_none()
        && stored_request.is_none()
        && stored_name.is_none();
    if cleanup_proven
        || (state != IntentState::Pending && !legacy_done_backfill)
        || stored_set.is_some_and(|stored| stored != scale_set_id)
        || stored_request.is_some_and(|stored| Some(stored) != request_id)
        || stored_name
            .as_deref()
            .is_some_and(|stored| stored != runner_name)
    {
        return Err(HostError::Journal);
    }
    let changed = connection
        .execute(
            "UPDATE intents SET scale_set_id = COALESCE(scale_set_id, ?1), runner_request_id = COALESCE(runner_request_id, ?2), runner_name = COALESCE(runner_name, ?3) WHERE id = ?4 AND kind = 'launch' AND cleanup_proven = 0 AND (scale_set_id IS NULL OR scale_set_id = ?1) AND (?2 IS NULL OR runner_request_id IS NULL OR runner_request_id = ?2) AND (runner_name IS NULL OR runner_name = ?3)",
            (scale_set_id, request_id, runner_name.to_owned(), id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    one_row(changed)
}

async fn assigned_launch_row(
    connection: &turso::Connection,
    subject: &str,
    scale_set_id: i64,
    request_id: i64,
    runner_name: &str,
) -> Result<(i64, bool), HostError> {
    let matches = matching_assigned_rows(connection, scale_set_id, request_id, runner_name).await?;
    let active: Vec<AssignedLaunchRow> = matches
        .iter()
        .filter(|row| !row.cleanup_proven)
        .copied()
        .collect();
    if active.len() > 1 {
        return Err(HostError::Journal);
    }
    let completed = completed_assignment(connection, scale_set_id, request_id, runner_name).await?;
    if let Some(active) = active.first().copied() {
        if completed.is_some_and(|id| id != active.id) {
            return Err(HostError::Journal);
        }
        if active.safe_retry {
            reset_failed_assignment(connection, active.id).await?;
            bind_identity_row(
                connection,
                active.id,
                scale_set_id,
                Some(request_id),
                runner_name,
            )
            .await?;
            return Ok((active.id, true));
        }
        bind_identity_row(
            connection,
            active.id,
            scale_set_id,
            Some(request_id),
            runner_name,
        )
        .await?;
        return Ok((active.id, false));
    }
    if let Some(cleaned) = matches
        .first()
        .filter(|row| row.cleanup_proven && row.identity_match)
    {
        if completed.is_some_and(|id| id != cleaned.id) {
            return Err(HostError::Journal);
        }
        return Ok((cleaned.id, false));
    }
    if let Some(id) = completed {
        return Ok((id, false));
    }
    connection
        .execute(
            "INSERT INTO intents (kind, subject, state, scale_set_id, runner_request_id, runner_name) VALUES ('launch', ?1, 'pending', ?2, ?3, ?4)",
            (subject, scale_set_id, request_id, runner_name),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok((connection.last_insert_rowid(), true))
}

#[derive(Debug, Clone, Copy)]
struct AssignedLaunchRow {
    id: i64,
    cleanup_proven: bool,
    safe_retry: bool,
    identity_match: bool,
}

async fn matching_assigned_rows(
    connection: &turso::Connection,
    scale_set_id: i64,
    request_id: i64,
    runner_name: &str,
) -> Result<Vec<AssignedLaunchRow>, HostError> {
    let mut rows = connection
        .query(
            "SELECT id, subject, state, cleanup_proven, docker_id, dind_id, worker_volume, scale_set_id, runner_request_id, runner_name, acquire_attempted, acquire_resolved, acquired, jit_requested FROM intents WHERE kind = 'launch' ORDER BY id",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut matching = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        if let Some(candidate) = assigned_row_match(&row, scale_set_id, request_id, runner_name)? {
            matching.push(candidate);
        }
    }
    Ok(matching)
}

fn assigned_row_match(
    row: &turso::Row,
    scale_set_id: i64,
    request_id: i64,
    runner_name: &str,
) -> Result<Option<AssignedLaunchRow>, HostError> {
    let id: i64 = row.get(0).map_err(|_| HostError::Journal)?;
    let subject: String = row.get(1).map_err(|_| HostError::Journal)?;
    let row_scale: Option<i64> = row.get(7).map_err(|_| HostError::Journal)?;
    let row_request: Option<i64> = row.get(8).map_err(|_| HostError::Journal)?;
    let row_name: Option<String> = row.get(9).map_err(|_| HostError::Journal)?;
    let same_request_subject = assigned_request(&subject) == Some(request_id);
    let same_set_request = row_scale == Some(scale_set_id) && row_request == Some(request_id);
    let same_set_name = row_scale == Some(scale_set_id) && row_name.as_deref() == Some(runner_name);
    if same_set_name
        && (row_request.is_some_and(|value| value != request_id)
            || assigned_request(&subject).is_some_and(|value| value != request_id))
    {
        return Err(HostError::Journal);
    }
    if row_scale.is_some_and(|value| value != scale_set_id) {
        if same_request_subject || row_request == Some(request_id) {
            return Err(HostError::Journal);
        }
        return Ok(None);
    }
    let legacy_subject = same_request_subject;
    if !same_set_request && !legacy_subject && !same_set_name {
        return Ok(None);
    }
    if row_request.is_some_and(|value| value != request_id)
        || row_name
            .as_deref()
            .is_some_and(|value| value != runner_name)
    {
        return Err(HostError::Journal);
    }
    let state_text: String = row.get(2).map_err(|_| HostError::Journal)?;
    let state = IntentState::parse(&state_text)?;
    let cleanup_proven: bool = row.get(3).map_err(|_| HostError::Journal)?;
    let docker_id: Option<String> = row.get(4).map_err(|_| HostError::Journal)?;
    let dind_id: Option<String> = row.get(5).map_err(|_| HostError::Journal)?;
    let worker_volume: Option<String> = row.get(6).map_err(|_| HostError::Journal)?;
    let attempted: bool = row.get(10).map_err(|_| HostError::Journal)?;
    let resolved: bool = row.get(11).map_err(|_| HostError::Journal)?;
    let acquired: bool = row.get(12).map_err(|_| HostError::Journal)?;
    let jit_requested: bool = row.get(13).map_err(|_| HostError::Journal)?;
    let safe_retry = state == IntentState::Failed
        && !cleanup_proven
        && docker_id.is_none()
        && dind_id.is_none()
        && worker_volume.is_none()
        && !attempted
        && !resolved
        && !acquired
        && !jit_requested;
    Ok(Some(AssignedLaunchRow {
        id,
        cleanup_proven,
        safe_retry,
        identity_match: same_set_request || same_set_name,
    }))
}

async fn completed_assignment(
    connection: &turso::Connection,
    scale_set_id: i64,
    request_id: i64,
    runner_name: &str,
) -> Result<Option<i64>, HostError> {
    let mut rows = connection
        .query(
            "SELECT intent_id, runner_name FROM completion_cleanup WHERE scale_set_id = ?1 AND runner_request_id = ?2 LIMIT 2",
            (scale_set_id, request_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let id: i64 = row.get(0).map_err(|_| HostError::Journal)?;
    let name: String = row.get(1).map_err(|_| HostError::Journal)?;
    if name != runner_name || rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(Some(id))
}

async fn reset_failed_assignment(connection: &turso::Connection, id: i64) -> Result<(), HostError> {
    let changed = connection
        .execute(
            "UPDATE intents SET state = 'pending', cleanup_proven = 0 WHERE id = ?1 AND kind = 'launch' AND state = 'failed' AND cleanup_proven = 0 AND docker_id IS NULL AND dind_id IS NULL AND worker_volume IS NULL AND acquire_attempted = 0 AND acquire_resolved = 0 AND acquired = 0 AND jit_requested = 0 AND NOT EXISTS (SELECT 1 FROM completion_cleanup WHERE intent_id = intents.id)",
            [id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    one_row(changed)
}

fn valid_launch_identity(subject: &str, request_id: Option<i64>, runner_name: &str) -> bool {
    if let Some(subject_request) = assigned_request(subject) {
        request_id == Some(subject_request) && runner_name == format!("v{subject_request}")
    } else {
        request_id.is_none()
            && (is_session_name(subject) || is_message_name(subject))
            && subject == runner_name
    }
}
