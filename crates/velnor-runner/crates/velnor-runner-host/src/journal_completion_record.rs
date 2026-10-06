//! Completion event identity binding.

use crate::error::HostError;
use crate::journal::{Journal, token_rejected};

use super::{assigned_request, finish_transaction, is_message_name, is_session_name};

impl Journal {
    /// Record one exact runner completion before acknowledging its queue message.
    ///
    /// Assigned legacy subjects are matched by request id and `v{request_id}`.
    /// Session launches are matched by persisted scale set and exact session name.
    /// `None` means no local launch matched; ambiguous or conflicting identities fail.
    pub(crate) async fn record_runner_completed(
        &self,
        scale_set_id: i64,
        request_id: i64,
        runner_id: i64,
        runner_name: &str,
    ) -> Result<Option<i64>, HostError> {
        if scale_set_id <= 0 || request_id <= 0 || runner_id <= 0 || token_rejected(runner_name) {
            return Err(HostError::Journal);
        }
        let connection = self.connection().await?;
        connection
            .execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = record_completion_row(
            &connection,
            scale_set_id,
            request_id,
            runner_id,
            runner_name,
        )
        .await;
        finish_transaction(&connection, result).await
    }
}

async fn record_completion_row(
    connection: &turso::Connection,
    scale_set_id: i64,
    request_id: i64,
    runner_id: i64,
    runner_name: &str,
) -> Result<Option<i64>, HostError> {
    if let Some(id) =
        prior_completion(connection, scale_set_id, request_id, runner_id, runner_name).await?
    {
        return Ok(Some(id));
    }
    let Some((id, assigned)) =
        completion_candidate(connection, scale_set_id, request_id, runner_name).await?
    else {
        return Ok(None);
    };
    verify_no_identity_collision(connection, id, scale_set_id, request_id, runner_name).await?;
    bind_completion_identity(
        connection,
        id,
        scale_set_id,
        request_id,
        runner_name,
        assigned,
    )
    .await?;
    let changed = connection
        .execute(
            "UPDATE intents SET github_runner_id = COALESCE(github_runner_id, ?1), runner_completed = 1 WHERE id = ?2 AND cleanup_proven = 0 AND (github_runner_id IS NULL OR github_runner_id = ?1)",
            (runner_id.to_string(), id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed != 1 {
        return Err(HostError::Journal);
    }
    connection
        .execute(
            "INSERT INTO completion_cleanup (intent_id, scale_set_id, runner_request_id, runner_id, runner_name) VALUES (?1, ?2, ?3, ?4, ?5)",
            (id, scale_set_id, request_id, runner_id, runner_name),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(Some(id))
}

async fn bind_completion_identity(
    connection: &turso::Connection,
    id: i64,
    scale_set_id: i64,
    request_id: i64,
    runner_name: &str,
    assigned: bool,
) -> Result<(), HostError> {
    let statement = if assigned {
        "UPDATE intents SET scale_set_id = COALESCE(scale_set_id, ?1), runner_request_id = COALESCE(runner_request_id, ?2), runner_name = COALESCE(runner_name, ?3) WHERE id = ?4 AND kind = 'launch' AND cleanup_proven = 0 AND (scale_set_id IS NULL OR scale_set_id = ?1) AND (runner_request_id IS NULL OR runner_request_id = ?2) AND (runner_name IS NULL OR runner_name = ?3)"
    } else {
        "UPDATE intents SET scale_set_id = ?1 WHERE id = ?4 AND kind = 'launch' AND cleanup_proven = 0 AND scale_set_id = ?1 AND runner_name = ?3"
    };
    let changed = connection
        .execute(
            statement,
            (scale_set_id, request_id, runner_name.to_owned(), id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed == 1 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

async fn prior_completion(
    connection: &turso::Connection,
    scale_set_id: i64,
    request_id: i64,
    runner_id: i64,
    runner_name: &str,
) -> Result<Option<i64>, HostError> {
    let mut rows = connection
        .query(
            "SELECT intent_id, scale_set_id, runner_request_id, runner_id, runner_name FROM completion_cleanup WHERE scale_set_id = ?1 AND (runner_request_id = ?2 OR runner_id = ?4 OR runner_name = ?3) ORDER BY intent_id LIMIT 2",
            (scale_set_id, request_id, runner_name, runner_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let id: i64 = row.get(0).map_err(|_| HostError::Journal)?;
    let exact = row.get::<i64>(1).map_err(|_| HostError::Journal)? == scale_set_id
        && row.get::<i64>(2).map_err(|_| HostError::Journal)? == request_id
        && row.get::<i64>(3).map_err(|_| HostError::Journal)? == runner_id
        && row.get::<String>(4).map_err(|_| HostError::Journal)? == runner_name;
    if !exact || rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(Some(id))
}

async fn completion_candidate(
    connection: &turso::Connection,
    scale_set_id: i64,
    request_id: i64,
    runner_name: &str,
) -> Result<Option<(i64, bool)>, HostError> {
    let mut rows = connection
        .query(
            "SELECT id, subject, scale_set_id, runner_request_id, runner_name FROM intents WHERE kind = 'launch' AND cleanup_proven = 0 ORDER BY id",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut candidate = None;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        if let Some(id) = candidate_row(&row, scale_set_id, request_id, runner_name)?
            && candidate.replace(id).is_some()
        {
            return Err(HostError::Journal);
        }
    }
    Ok(candidate)
}

fn candidate_row(
    row: &turso::Row,
    scale_set_id: i64,
    request_id: i64,
    runner_name: &str,
) -> Result<Option<(i64, bool)>, HostError> {
    let id: i64 = row.get(0).map_err(|_| HostError::Journal)?;
    let subject: String = row.get(1).map_err(|_| HostError::Journal)?;
    let row_scale: Option<i64> = row.get(2).map_err(|_| HostError::Journal)?;
    let row_request: Option<i64> = row.get(3).map_err(|_| HostError::Journal)?;
    let row_name: Option<String> = row.get(4).map_err(|_| HostError::Journal)?;
    let assigned = assigned_request(&subject) == Some(request_id);
    let named = (is_session_name(&subject) || is_message_name(&subject)) && subject == runner_name;
    if !assigned && !named {
        return Ok(None);
    }
    if row_scale.is_some_and(|value| value != scale_set_id) {
        return Ok(None);
    }
    let assignment_name = format!("v{request_id}");
    if assigned && runner_name != assignment_name
        || row_name
            .as_deref()
            .is_some_and(|value| value != runner_name)
    {
        return Err(HostError::Journal);
    }
    if named && (row_scale != Some(scale_set_id) || row_name.as_deref() != Some(runner_name)) {
        return Err(HostError::Journal);
    }
    if row_request.is_some_and(|value| value != request_id) {
        return Err(HostError::Journal);
    }
    Ok(Some((id, assigned)))
}

async fn verify_no_identity_collision(
    connection: &turso::Connection,
    candidate: i64,
    scale_set_id: i64,
    request_id: i64,
    runner_name: &str,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT id FROM intents WHERE kind = 'launch' AND cleanup_proven = 0 AND ((scale_set_id = ?1 AND runner_request_id = ?2) OR (scale_set_id = ?1 AND runner_name = ?3)) ORDER BY id",
            (scale_set_id, request_id, runner_name),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let id: i64 = row.get(0).map_err(|_| HostError::Journal)?;
        if id != candidate {
            return Err(HostError::Journal);
        }
    }
    Ok(())
}
