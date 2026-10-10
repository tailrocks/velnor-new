//! Local turso journal. Each step opens its own connection and commits
//! before the caller runs an external effect.

use std::ops::AsyncFnOnce;
use std::path::Path;

use crate::error::HostError;
use crate::reconcile::IntentRow;
use file_identity::JournalFile;

#[path = "journal_completion.rs"]
mod completion;
mod file_identity;
mod launch;
mod read;
mod resource_probe;
mod schema;
mod sql;
mod transaction;
mod worker_volume;

#[cfg(test)]
mod schema_metadata_tests;
pub(crate) use completion::{
    CleanupClaim, CompletedLaunch, CompletionIdentity, CompletionInboxEntry,
    MAX_COMPLETION_BODY_BYTES, MAX_COMPLETION_INBOX_SCAN,
};
use sql::{one_row, token_rejected};

pub(crate) use resource_probe::{ProbePhase, ProbeRow, ProbeSeed};

/// Durable intent row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntentState {
    /// Written before the effect.
    Pending,
    /// The effect finished.
    Done,
    /// The effect may have happened.
    Uncertain,
    /// The effect definitely did not happen.
    Failed,
}

impl IntentState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Done => "done",
            Self::Uncertain => "uncertain",
            Self::Failed => "failed",
        }
    }

    pub(crate) fn parse(text: &str) -> Result<Self, HostError> {
        match text {
            "pending" => Ok(Self::Pending),
            "done" => Ok(Self::Done),
            "uncertain" => Ok(Self::Uncertain),
            "failed" => Ok(Self::Failed),
            _ => Err(HostError::Journal),
        }
    }
}

/// Outcome of an effect that already ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The effect finished.
    Done,
    /// Transport was ambiguous. Capacity stays.
    Uncertain,
    /// The service rejected the call.
    DefiniteFailure,
}

/// File-backed journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Journal {
    file: JournalFile,
}

impl Journal {
    /// Create the file and schema.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when turso cannot open the path.
    pub async fn open(path: &Path) -> Result<Self, HostError> {
        let journal = Self {
            file: JournalFile::open(path).await?,
        };
        journal.bootstrap().await?;
        Ok(journal)
    }

    /// Insert a pending intent and commit before returning.
    ///
    /// The same `kind` and `subject` reuse the live row. A failed or cleaned row starts a new id.
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
    /// Rows with proven cleanup are terminal and cannot be finished again.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the id is missing, cleanup was proven, or the write fails.
    pub async fn finish(&self, id: i64, outcome: Outcome) -> Result<(), HostError> {
        let state = match outcome {
            Outcome::Done => IntentState::Done,
            Outcome::Uncertain => IntentState::Uncertain,
            Outcome::DefiniteFailure => IntentState::Failed,
        };
        let conn = self.connection().await?;
        transaction::with_unique_id(&conn, id, async move |conn| {
            let changed = conn
                .execute(
                    "UPDATE intents SET state = ?1 WHERE id = ?2 AND cleanup_proven = 0",
                    (state.as_str().to_owned(), id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            one_row(changed)
        })
        .await
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

    /// Store plain docker and GitHub runner ids after the effect returns.
    ///
    /// `None` leaves the existing column. Empty ids are rejected.
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
        let docker_id = docker_id.map(str::to_owned);
        let github_runner_id = github_runner_id.map(str::to_owned);
        transaction::with_unique_id(&conn, id, async move |conn| {
            let changed = conn
                .execute(
                    "UPDATE intents SET docker_id = COALESCE(?1, docker_id), github_runner_id = COALESCE(?2, github_runner_id) WHERE id = ?3",
                    (docker_id, github_runner_id, id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            one_row(changed)
        })
        .await
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
        let runner_id = runner_id.map(str::to_owned);
        let dind_id = dind_id.map(str::to_owned);
        transaction::with_unique_id(&conn, id, async move |conn| {
            let changed = conn
                .execute(
                    "UPDATE intents SET docker_id = COALESCE(docker_id, ?1), dind_id = COALESCE(dind_id, ?2) WHERE id = ?3 AND (?1 IS NULL OR docker_id IS NULL OR docker_id = ?1) AND (?2 IS NULL OR dind_id IS NULL OR dind_id = ?2)",
                    (runner_id.clone(), dind_id.clone(), id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            match changed {
                1 => Ok(()),
                0 => same_ids(conn, id, runner_id.as_deref(), dind_id.as_deref()).await,
                _ => Err(HostError::Journal),
            }
        })
        .await
    }

    /// Record that cleanup of this row's ids is proven.
    ///
    /// Completion rows require a live completion claim and verified runner absence.
    /// Use the completion-specific proof method for those rows.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the row is missing.
    pub async fn record_cleanup(&self, id: i64) -> Result<(), HostError> {
        let conn = self.connection().await?;
        transaction::with_unique_id(&conn, id, async move |conn| {
            let changed = conn
                .execute(
                    "UPDATE intents SET cleanup_proven = 1 WHERE id = ?1 AND NOT EXISTS (SELECT 1 FROM completion_cleanup WHERE intent_id = ?1)",
                    [id],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            one_row(changed)
        })
        .await
    }

    async fn bootstrap(&self) -> Result<(), HostError> {
        let conn = self.connection().await?;
        schema::bootstrap(&conn).await
    }

    pub(super) async fn connection(&self) -> Result<turso::Connection, HostError> {
        self.file.connection().await
    }

    pub(super) fn path(&self) -> &Path {
        self.file.path()
    }

    pub(crate) fn state_directory(&self) -> Result<&Path, HostError> {
        self.path().parent().ok_or(HostError::Path)
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
    conn.execute(
        "INSERT INTO intents (kind, subject, state) VALUES (?1, ?2, 'pending')",
        (kind.to_owned(), subject.to_owned()),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    Ok(conn.last_insert_rowid())
}

async fn live_id(
    conn: &turso::Connection,
    kind: &str,
    subject: &str,
) -> Result<Option<i64>, HostError> {
    let mut rows = conn
        .query(
            "SELECT id FROM intents WHERE kind = ?1 AND subject = ?2 AND state != 'failed' AND cleanup_proven = 0 ORDER BY id DESC LIMIT 1",
            (kind.to_owned(), subject.to_owned()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let id: i64 = row.get(0).map_err(|_| HostError::Journal)?;
    Ok(Some(id))
}

fn intent_row(row: &turso::Row) -> Result<IntentRow, HostError> {
    let state_text: String = row.get(3).map_err(|_| HostError::Journal)?;
    Ok(IntentRow {
        id: row.get(0).map_err(|_| HostError::Journal)?,
        kind: row.get(1).map_err(|_| HostError::Journal)?,
        subject: row.get(2).map_err(|_| HostError::Journal)?,
        state: IntentState::parse(&state_text)?,
        docker_id: row.get(4).map_err(|_| HostError::Journal)?,
        dind_id: row.get(7).map_err(|_| HostError::Journal)?,
        worker_volume: row.get(8).map_err(|_| HostError::Journal)?,
        scale_set_id: row.get(9).map_err(|_| HostError::Journal)?,
        request_id: row.get(10).map_err(|_| HostError::Journal)?,
        runner_name: row.get(11).map_err(|_| HostError::Journal)?,
        docker_engine_id: row.get(12).map_err(|_| HostError::Journal)?,
        launch_phase: row
            .get::<Option<String>>(13)
            .map_err(|_| HostError::Journal)?
            .as_deref()
            .map(crate::reconcile::LaunchPhase::parse)
            .transpose()?,
        github_runner_id: row.get(5).map_err(|_| HostError::Journal)?,
        cleanup_proven: row.get(6).map_err(|_| HostError::Journal)?,
        launch_id: row.get(14).map_err(|_| HostError::Journal)?,
        assignment_key: row.get(15).map_err(|_| HostError::Journal)?,
        seed_generation_id: row.get(16).map_err(|_| HostError::Journal)?,
        acquire_attempted: row.get(17).map_err(|_| HostError::Journal)?,
        acquire_resolved: row.get(18).map_err(|_| HostError::Journal)?,
        acquired: row.get(19).map_err(|_| HostError::Journal)?,
        jit_requested: row.get(20).map_err(|_| HostError::Journal)?,
        runner_completed: row.get(21).map_err(|_| HostError::Journal)?,
    })
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
