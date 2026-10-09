//! Durable general-purpose intent operations and row decoding.

use std::ops::AsyncFnOnce;

use crate::error::HostError;
use crate::reconcile::IntentRow;

use super::{IntentState, Journal, Outcome, live_id, one_row, token_rejected};

pub(super) const INTENT_COLUMNS: &str = "id, kind, subject, state, docker_id, github_runner_id, cleanup_proven, dind_id, worker_volume, message_id, runner_request_id, requested_workflow_run_id, requested_job_id, runner_name, observed_job_id, observed_workflow_run_id, remote_terminal, effect_state, outer_network_name, outer_network_id, runner_start_state, observed_actions_attempt, observed_actions_job_id, observed_actions_conclusion";

impl Journal {
    /// Insert a pending intent and commit before returning.
    ///
    /// The same `kind` and `subject` reuse the live row. A failed row starts a new id.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] on I/O failure or a rejected kind or subject.
    pub async fn begin(&self, kind: &str, subject: &str) -> Result<i64, HostError> {
        if token_rejected(kind) || token_rejected(subject) {
            return Err(HostError::Journal);
        }
        let conn = self.connection().await?;
        commit_live(&conn, kind, subject).await
    }

    /// Record the outcome of exactly one row on a new connection.
    ///
    /// Scoped launch rows have monotonic outcomes. `Pending` may advance to any
    /// result, and `Uncertain` may later become `Done`; a generic result cannot
    /// rewrite `Uncertain` or `Done` to `Failed`. `Done` and `Uncertain` also
    /// persist `effect_state = 'may_have_effect'`. A scoped definite no-effect
    /// result must use [`Journal::record_launch_no_effect`], which validates the
    /// durable pre-effect state atomically. Legacy rows retain their historical
    /// transition behavior and always occupy scoped capacity accounting.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the id is missing or the write fails.
    pub async fn finish(&self, id: i64, outcome: Outcome) -> Result<(), HostError> {
        let state = match outcome {
            Outcome::Done => IntentState::Done,
            Outcome::Uncertain => IntentState::Uncertain,
            Outcome::DefiniteFailure => IntentState::Failed,
        };
        let conn = self.connection().await?;
        let state_text = state.as_str().to_owned();
        let changed = conn
            .execute(
                "UPDATE intents SET state = ?1, effect_state = CASE WHEN replay_key_version = 1 AND ?1 IN ('done', 'uncertain') THEN 'may_have_effect' ELSE effect_state END WHERE id = ?2 AND (replay_key_version != 1 OR state = ?1 OR state = 'pending' OR (state = 'uncertain' AND ?1 = 'done')) AND kind != 'discovery-credential'",
                (state_text, id),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        one_row(changed)
    }

    /// Read a row back, including after a new [`Journal::open`].
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the row is missing.
    pub async fn read(&self, id: i64) -> Result<IntentState, HostError> {
        let conn = self.connection().await?;
        let mut rows = conn
            .query("SELECT state FROM intents WHERE id = ?1", [id])
            .await
            .map_err(|_| HostError::Journal)?;
        let row = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?;
        let text: String = row.get(0).map_err(|_| HostError::Journal)?;
        IntentState::parse(&text)
    }

    /// Commit pending, run `effect` with no connection, then [`Journal::finish`].
    ///
    /// `secret` is passed only to `effect` and is not written.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the intent cannot be stored or finished.
    pub async fn around<F>(
        &self,
        kind: &str,
        subject: &str,
        secret: &str,
        effect: F,
    ) -> Result<i64, HostError>
    where
        F: AsyncFnOnce(&str) -> Outcome,
    {
        let id = self.begin(kind, subject).await?;
        let outcome = effect(secret).await;
        self.finish(id, outcome).await?;
        Ok(id)
    }

    /// Bind plain Docker and GitHub runner ids after the effect returns.
    ///
    /// `None` leaves the existing column. Once present, each id is immutable;
    /// cleanup and start evidence must continue to refer to the same generation.
    /// Empty ids are rejected.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the row is missing or an id is rejected.
    pub async fn bind(
        &self,
        id: i64,
        docker_id: Option<&str>,
        github_runner_id: Option<&str>,
    ) -> Result<(), HostError> {
        if docker_id.is_some_and(token_rejected) || github_runner_id.is_some_and(token_rejected) {
            return Err(HostError::Journal);
        }
        let conn = self.connection().await?;
        let changed = conn
            .execute(
                "UPDATE intents SET docker_id = COALESCE(docker_id, ?1), github_runner_id = COALESCE(github_runner_id, ?2) WHERE id = ?3 AND (?1 IS NULL OR docker_id IS NULL OR docker_id = ?1) AND (?2 IS NULL OR github_runner_id IS NULL OR github_runner_id = ?2) AND NOT EXISTS (SELECT 1 FROM worker_cleanup_steps WHERE launch_id = ?3 AND step_key = 'outer-network-removal')",
                (
                    docker_id.map(str::to_owned),
                    github_runner_id.map(str::to_owned),
                    id,
                ),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        match changed {
            1 => Ok(()),
            0 if !super::outer_network_removal_started(&conn, id).await? => {
                same_bind_ids(&conn, id, docker_id, github_runner_id).await
            }
            _ => Err(HostError::Journal),
        }
    }

    /// Store the runner id and the private `DinD` id. `None` keeps the column.
    ///
    /// Commit this before the container is started. Empty ids are rejected.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the row is missing or an id is rejected.
    pub async fn bind_worker(
        &self,
        id: i64,
        runner_id: Option<&str>,
        dind_id: Option<&str>,
    ) -> Result<(), HostError> {
        if runner_id.is_some_and(token_rejected) || dind_id.is_some_and(token_rejected) {
            return Err(HostError::Journal);
        }
        let conn = self.connection().await?;
        let changed = conn
            .execute(
                "UPDATE intents SET docker_id = COALESCE(docker_id, ?1), dind_id = COALESCE(dind_id, ?2) WHERE id = ?3 AND (?1 IS NULL OR docker_id IS NULL OR docker_id = ?1) AND (?2 IS NULL OR dind_id IS NULL OR dind_id = ?2) AND NOT EXISTS (SELECT 1 FROM worker_cleanup_steps WHERE launch_id = ?3 AND step_key = 'outer-network-removal')",
                (
                    runner_id.map(str::to_owned),
                    dind_id.map(str::to_owned),
                    id,
                ),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        match changed {
            1 => Ok(()),
            0 if !super::outer_network_removal_started(&conn, id).await? => {
                same_ids(&conn, id, runner_id, dind_id).await
            }
            _ => Err(HostError::Journal),
        }
    }

    /// Mark a non-launch intent cleaned after its operation-specific cleanup.
    ///
    /// Launch generations require [`Journal::record_physical_cleanup`]; this
    /// legacy marker carries no container, diagnostics, or ownership proof and
    /// therefore cannot release a launch reservation.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the row is missing.
    pub async fn record_cleanup(&self, id: i64) -> Result<(), HostError> {
        let conn = self.connection().await?;
        let changed = conn
            .execute(
                "UPDATE intents SET cleanup_proven = 1 WHERE id = ?1 AND kind NOT IN ('launch', 'discovery-credential')",
                [id],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        one_row(changed)
    }

    /// Load every row. The connection closes before this returns.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] on I/O or a corrupt state.
    pub async fn rows(&self) -> Result<Vec<IntentRow>, HostError> {
        let conn = self.connection().await?;
        let mut query = conn
            .query(
                &format!("SELECT {INTENT_COLUMNS} FROM intents ORDER BY id"),
                (),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let mut out = Vec::new();
        while let Some(row) = query.next().await.map_err(|_| HostError::Journal)? {
            out.push(intent_row(&row)?);
        }
        Ok(out)
    }
}

async fn same_bind_ids(
    conn: &turso::Connection,
    id: i64,
    docker_id: Option<&str>,
    github_runner_id: Option<&str>,
) -> Result<(), HostError> {
    let mut rows = conn
        .query(
            "SELECT docker_id, github_runner_id FROM intents WHERE id = ?1",
            [id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Err(HostError::Journal);
    };
    let existing_docker: Option<String> = row.get(0).map_err(|_| HostError::Journal)?;
    let existing_runner: Option<String> = row.get(1).map_err(|_| HostError::Journal)?;
    let exact = docker_id.is_none_or(|value| existing_docker.as_deref() == Some(value))
        && github_runner_id.is_none_or(|value| existing_runner.as_deref() == Some(value));
    if exact {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

async fn commit_live(
    conn: &turso::Connection,
    kind: &str,
    subject: &str,
) -> Result<i64, HostError> {
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
) -> Result<i64, HostError> {
    if let Some(id) = live_id(conn, kind, subject).await? {
        return Ok(id);
    }
    if kind == "launch" {
        conn.execute(
            "INSERT INTO intents (kind, subject, state, runner_start_state) VALUES (?1, ?2, 'pending', 'not_requested')",
            (kind.to_owned(), subject.to_owned()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    } else {
        conn.execute(
            "INSERT INTO intents (kind, subject, state) VALUES (?1, ?2, 'pending')",
            (kind.to_owned(), subject.to_owned()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    }
    Ok(conn.last_insert_rowid())
}

pub(super) fn intent_row(row: &turso::Row) -> Result<IntentRow, HostError> {
    let state_text: String = row.get(3).map_err(|_| HostError::Journal)?;
    let observed_actions_attempt: Option<i64> = row.get(21).map_err(|_| HostError::Journal)?;
    let observed_actions_job_id: Option<i64> = row.get(22).map_err(|_| HostError::Journal)?;
    let observed_actions_conclusion: Option<String> =
        row.get(23).map_err(|_| HostError::Journal)?;
    if observed_actions_attempt.is_some_and(|value| value <= 0)
        || observed_actions_job_id.is_some_and(|value| value <= 0)
        || observed_actions_attempt.is_some() != observed_actions_job_id.is_some()
        || observed_actions_conclusion.as_deref().is_some_and(|value| {
            value.is_empty() || value.len() > 128 || value.chars().any(char::is_control)
        })
        || (observed_actions_conclusion.is_some() && observed_actions_job_id.is_none())
    {
        return Err(HostError::Journal);
    }
    Ok(IntentRow {
        id: row.get(0).map_err(|_| HostError::Journal)?,
        kind: row.get(1).map_err(|_| HostError::Journal)?,
        subject: row.get(2).map_err(|_| HostError::Journal)?,
        state: IntentState::parse(&state_text)?,
        launch_effect: crate::journal::LaunchEffectState::parse(
            &row.get::<String>(17).map_err(|_| HostError::Journal)?,
        )?,
        docker_id: row.get(4).map_err(|_| HostError::Journal)?,
        dind_id: row.get(7).map_err(|_| HostError::Journal)?,
        worker_volume: row.get(8).map_err(|_| HostError::Journal)?,
        github_runner_id: row.get(5).map_err(|_| HostError::Journal)?,
        message_id: row.get(9).map_err(|_| HostError::Journal)?,
        runner_request_id: row.get(10).map_err(|_| HostError::Journal)?,
        requested_workflow_run_id: row.get(11).map_err(|_| HostError::Journal)?,
        requested_job_id: row.get(12).map_err(|_| HostError::Journal)?,
        runner_name: row.get(13).map_err(|_| HostError::Journal)?,
        observed_job_id: row.get(14).map_err(|_| HostError::Journal)?,
        observed_workflow_run_id: row.get(15).map_err(|_| HostError::Journal)?,
        remote_terminal: match row.get::<i64>(16).map_err(|_| HostError::Journal)? {
            0 => false,
            1 => true,
            _ => return Err(HostError::Journal),
        },
        cleanup_proven: row.get(6).map_err(|_| HostError::Journal)?,
        outer_network_name: row.get(18).map_err(|_| HostError::Journal)?,
        outer_network_id: row.get(19).map_err(|_| HostError::Journal)?,
        runner_start_intent: crate::journal::RunnerStartIntent::parse(
            &row.get::<String>(20).map_err(|_| HostError::Journal)?,
        )?,
        observed_actions_attempt,
        observed_actions_job_id,
        observed_actions_conclusion,
    })
}

pub(super) async fn read_intent_row(
    conn: &turso::Connection,
    id: i64,
) -> Result<IntentRow, HostError> {
    let mut rows = conn
        .query(
            &format!("SELECT {INTENT_COLUMNS} FROM intents WHERE id = ?1"),
            [id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    intent_row(&row)
}

async fn same_ids(
    conn: &turso::Connection,
    id: i64,
    runner_id: Option<&str>,
    dind_id: Option<&str>,
) -> Result<(), HostError> {
    let mut rows = conn
        .query("SELECT docker_id, dind_id FROM intents WHERE id = ?1", [id])
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Err(HostError::Journal);
    };
    let runner: Option<String> = row.get(0).map_err(|_| HostError::Journal)?;
    let dind: Option<String> = row.get(1).map_err(|_| HostError::Journal)?;
    let kept = runner_id.is_none_or(|want| runner.as_deref() == Some(want))
        && dind_id.is_none_or(|want| dind.as_deref() == Some(want));
    if kept {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}
