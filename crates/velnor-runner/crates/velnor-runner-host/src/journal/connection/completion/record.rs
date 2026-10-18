//! Scope completion events and due cleanup rows to one repository.

use crate::error::HostError;
use crate::journal::sql::{intent_row, token_rejected};
use crate::journal::{CompletedLaunch, CompletionIdentity};

use super::super::ConnectionJournal;
use super::super::storage::{in_transaction, read_transaction};

impl ConnectionJournal {
    /// Record one runner completion before acknowledging its queue message.
    pub(crate) async fn record_runner_completed(
        &self,
        request_id: i64,
        runner_id: i64,
        runner_name: &str,
    ) -> Result<Option<i64>, HostError> {
        if request_id <= 0 || runner_id <= 0 || token_rejected(runner_name) {
            return Err(HostError::Journal);
        }
        in_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            let set_id = self.required_set_id(connection).await?;
            if let Some(id) = prior_completion(
                connection,
                self.connection_id.as_str(),
                set_id,
                request_id,
                runner_id,
                runner_name,
            )
            .await?
            {
                return Ok(Some(id));
            }
            let Some((id, assigned)) = completion_candidate(
                connection,
                self.connection_id.as_str(),
                set_id,
                request_id,
                runner_name,
            )
            .await?
            else {
                return Ok(None);
            };
            verify_no_identity_collision(
                connection,
                self.connection_id.as_str(),
                id,
                set_id,
                request_id,
                runner_name,
            )
            .await?;
            bind_completion_identity(
                connection,
                self.connection_id.as_str(),
                id,
                set_id,
                request_id,
                runner_name,
                assigned,
            )
            .await?;
            let changed = connection
                .execute(
                    "UPDATE connection_intents SET github_runner_id = COALESCE(github_runner_id, ?1), runner_completed = 1 WHERE id = ?2 AND connection_id = ?3 AND scale_set_id = ?4 AND kind = 'launch' AND cleanup_proven = 0 AND (github_runner_id IS NULL OR github_runner_id = ?1)",
                    (runner_id.to_string(), id, self.connection_id.as_str(), set_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Err(HostError::Journal);
            }
            connection
                .execute(
                    "INSERT INTO connection_completion_cleanup (intent_id, connection_id, scale_set_id, runner_request_id, runner_id, runner_name) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    (id, self.connection_id.as_str(), set_id, request_id, runner_id, runner_name),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            Ok(Some(id))
        })
        .await
    }

    /// Read this connection's due runner completions in retry order.
    pub(crate) async fn due_completed_launches(
        &self,
        now: i64,
        limit: u32,
    ) -> Result<Vec<CompletedLaunch>, HostError> {
        if now < 0 {
            return Err(HostError::Journal);
        }
        read_transaction(&self.journal, async |connection| {
            self.validate_scope(connection).await?;
            let set_id = self.required_set_id(connection).await?;
            let mut rows = connection
                .query(
                    "SELECT i.id, i.kind, i.subject, i.state, i.docker_id, i.github_runner_id, i.cleanup_proven, i.dind_id, i.worker_volume, i.scale_set_id, i.runner_request_id, i.runner_name, i.docker_engine_id, i.launch_phase, i.launch_id, i.assignment_key, i.seed_generation_id, i.acquire_attempted, i.acquire_resolved, i.acquired, i.jit_requested, i.runner_completed, c.scale_set_id, c.runner_request_id, c.runner_id, c.runner_name, c.runner_absent FROM connection_intents AS i JOIN connection_completion_cleanup AS c ON c.intent_id = i.id AND c.connection_id = i.connection_id WHERE i.connection_id = ?1 AND i.scale_set_id = ?2 AND i.kind = 'launch' AND i.cleanup_proven = 0 AND i.scale_set_id = c.scale_set_id AND i.runner_name = c.runner_name AND (i.runner_request_id IS NULL OR i.runner_request_id = c.runner_request_id) AND c.retry_after <= ?3 AND c.lease_until <= ?3 ORDER BY c.retry_after, i.id LIMIT ?4",
                    (self.connection_id.as_str(), set_id, now, i64::from(limit)),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let mut out = Vec::new();
            while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
                out.push(CompletedLaunch {
                    intent: intent_row(&row)?,
                    identity: CompletionIdentity {
                        scale_set_id: row.get(22).map_err(|_| HostError::Journal)?,
                        runner_request_id: row.get(23).map_err(|_| HostError::Journal)?,
                        runner_id: row.get(24).map_err(|_| HostError::Journal)?,
                        runner_name: row.get(25).map_err(|_| HostError::Journal)?,
                        runner_absent: row.get(26).map_err(|_| HostError::Journal)?,
                    },
                });
            }
            Ok(out)
        })
        .await
    }
}

async fn bind_completion_identity(
    connection: &turso::Connection,
    connection_id: &str,
    id: i64,
    scale_set_id: i64,
    request_id: i64,
    runner_name: &str,
    assigned: bool,
) -> Result<(), HostError> {
    let statement = if assigned {
        "UPDATE connection_intents SET runner_request_id = COALESCE(runner_request_id, ?1), runner_name = COALESCE(runner_name, ?2) WHERE id = ?3 AND connection_id = ?4 AND scale_set_id = ?5 AND kind = 'launch' AND cleanup_proven = 0 AND (runner_request_id IS NULL OR runner_request_id = ?1) AND (runner_name IS NULL OR runner_name = ?2)"
    } else {
        "UPDATE connection_intents SET scale_set_id = ?5 WHERE id = ?3 AND connection_id = ?4 AND scale_set_id = ?5 AND kind = 'launch' AND cleanup_proven = 0 AND runner_name = ?2"
    };
    let changed = connection
        .execute(
            statement,
            (request_id, runner_name, id, connection_id, scale_set_id),
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
    connection_id: &str,
    scale_set_id: i64,
    request_id: i64,
    runner_id: i64,
    runner_name: &str,
) -> Result<Option<i64>, HostError> {
    let mut rows = connection
        .query(
            "SELECT intent_id, scale_set_id, runner_request_id, runner_id, runner_name FROM connection_completion_cleanup WHERE connection_id = ?1 AND scale_set_id = ?2 AND (runner_request_id = ?3 OR runner_id = ?5 OR runner_name = ?4) ORDER BY intent_id LIMIT 2",
            (connection_id, scale_set_id, request_id, runner_name, runner_id),
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
    connection_id: &str,
    scale_set_id: i64,
    request_id: i64,
    runner_name: &str,
) -> Result<Option<(i64, bool)>, HostError> {
    let mut rows = connection
        .query(
            "SELECT id, subject, runner_request_id, runner_name FROM connection_intents WHERE connection_id = ?1 AND scale_set_id = ?2 AND kind = 'launch' AND cleanup_proven = 0 ORDER BY id",
            (connection_id, scale_set_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut candidate = None;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let id: i64 = row.get(0).map_err(|_| HostError::Journal)?;
        let subject: String = row.get(1).map_err(|_| HostError::Journal)?;
        let stored_request: Option<i64> = row.get(2).map_err(|_| HostError::Journal)?;
        let stored_name: Option<String> = row.get(3).map_err(|_| HostError::Journal)?;
        let assigned =
            stored_request == Some(request_id) || assigned_request(&subject) == Some(request_id);
        let named = stored_name.as_deref() == Some(runner_name)
            || (is_subject_name(&subject) && subject == runner_name);
        if !assigned && !named {
            continue;
        }
        if assigned && runner_name != format!("v{request_id}")
            || stored_request.is_some_and(|value| value != request_id)
            || stored_name
                .as_deref()
                .is_some_and(|value| value != runner_name)
            || (named && stored_name.as_deref() != Some(runner_name))
        {
            return Err(HostError::Journal);
        }
        if candidate.replace((id, assigned)).is_some() {
            return Err(HostError::Journal);
        }
    }
    Ok(candidate)
}

async fn verify_no_identity_collision(
    connection: &turso::Connection,
    connection_id: &str,
    candidate: i64,
    scale_set_id: i64,
    request_id: i64,
    runner_name: &str,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT id FROM connection_intents WHERE connection_id = ?1 AND scale_set_id = ?2 AND kind = 'launch' AND cleanup_proven = 0 AND (runner_request_id = ?3 OR runner_name = ?4) ORDER BY id",
            (connection_id, scale_set_id, request_id, runner_name),
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

fn assigned_request(subject: &str) -> Option<i64> {
    let body = subject.strip_prefix('m')?;
    let (message, request) = body.split_once('r')?;
    if message.is_empty()
        || request.is_empty()
        || !message.bytes().all(|byte| byte.is_ascii_digit())
        || !request.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let message_id = message.parse::<i64>().ok()?;
    let request_id = request.parse::<i64>().ok()?;
    (message_id >= 0
        && request_id > 0
        && message == message_id.to_string()
        && request == request_id.to_string())
    .then_some(request_id)
}

fn is_subject_name(subject: &str) -> bool {
    subject.strip_prefix('s').is_some_and(|suffix| {
        (2..=13).contains(&subject.len())
            && !suffix.is_empty()
            && suffix.bytes().all(|byte| byte.is_ascii_alphanumeric())
    }) || subject.strip_prefix('m').is_some_and(|suffix| {
        suffix
            .parse::<i64>()
            .is_ok_and(|value| suffix == value.to_string())
    })
}
