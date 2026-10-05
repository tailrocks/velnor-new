//! Local turso journal. Each step opens its own connection and commits
//! before the caller runs an external effect.

use std::ops::AsyncFnOnce;
use std::path::{Path, PathBuf};

use crate::error::HostError;
use crate::reconcile::IntentRow;

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

    fn parse(text: &str) -> Result<Self, HostError> {
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
    path: PathBuf,
}

impl Journal {
    /// Create the file and schema.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when turso cannot open the path.
    pub async fn open(path: &Path) -> Result<Self, HostError> {
        let journal = Self {
            path: path.to_path_buf(),
        };
        journal.bootstrap().await?;
        Ok(journal)
    }

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
        let changed = conn
            .execute(
                "UPDATE intents SET state = ?1 WHERE id = ?2",
                (state.as_str().to_owned(), id),
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
        let changed = conn
            .execute(
                "UPDATE intents SET docker_id = COALESCE(?1, docker_id), github_runner_id = COALESCE(?2, github_runner_id) WHERE id = ?3",
                (
                    docker_id.map(str::to_owned),
                    github_runner_id.map(str::to_owned),
                    id,
                ),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        one_row(changed)
    }

    /// Record that cleanup of this row's ids is proven.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the row is missing.
    pub async fn record_cleanup(&self, id: i64) -> Result<(), HostError> {
        let conn = self.connection().await?;
        let changed = conn
            .execute("UPDATE intents SET cleanup_proven = 1 WHERE id = ?1", [id])
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
                "SELECT id, kind, subject, state, docker_id, github_runner_id, cleanup_proven FROM intents ORDER BY id",
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

    async fn bootstrap(&self) -> Result<(), HostError> {
        let conn = self.connection().await?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS intents (id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, subject TEXT NOT NULL, state TEXT NOT NULL, docker_id TEXT, github_runner_id TEXT, cleanup_proven INTEGER NOT NULL DEFAULT 0)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
        Ok(())
    }

    async fn connection(&self) -> Result<turso::Connection, HostError> {
        let text = self.path.to_str().ok_or(HostError::Path)?;
        let db = turso::Builder::new_local(text)
            .build()
            .await
            .map_err(|_| HostError::Journal)?;
        db.connect().map_err(|_| HostError::Journal)
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
            "SELECT id FROM intents WHERE kind = ?1 AND subject = ?2 AND state != 'failed' ORDER BY id LIMIT 1",
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
        github_runner_id: row.get(5).map_err(|_| HostError::Journal)?,
        cleanup_proven: row.get(6).map_err(|_| HostError::Journal)?,
    })
}

fn one_row(changed: u64) -> Result<(), HostError> {
    if changed == 1 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

fn token_rejected(token: &str) -> bool {
    token.is_empty() || token.chars().any(|ch| matches!(ch, '\'' | '"'))
}
