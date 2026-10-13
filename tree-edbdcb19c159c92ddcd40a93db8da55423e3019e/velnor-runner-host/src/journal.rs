//! Local turso journal. Each step opens its own connection and commits
//! before the caller runs an external effect.

use std::ops::AsyncFnOnce;
use std::path::{Path, PathBuf};

use crate::error::HostError;
use crate::journal_assignment::commit_assignment;
use crate::journal_schema;
use crate::journal_sql::{commit_live, intent_row, one_row, token_rejected};
use crate::reconcile::IntentRow;

pub use crate::launch_identity::LaunchIdentity;

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

    pub(super) fn parse(text: &str) -> Result<Self, HostError> {
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

    /// Begin or replay one acquired scale-set request.
    ///
    /// `runnerRequestId` is stable across queue redelivery. `message_id` is
    /// used only to adopt rows written by the old message-based key.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the request id is ambiguous or the write fails.
    pub async fn begin_assignment(
        &self,
        set_id: i64,
        request_id: i64,
        message_id: i64,
    ) -> Result<i64, HostError> {
        if set_id <= 0 || request_id < 0 || message_id < 0 {
            return Err(HostError::Journal);
        }
        let conn = self.connection().await?;
        commit_assignment(&conn, set_id, request_id, message_id).await
    }

    /// Bind this journal to one Docker engine. A changed engine fails closed.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the id is invalid or changes.
    pub async fn bind_engine(&self, engine_id: &str) -> Result<(), HostError> {
        let conn = self.connection().await?;
        journal_schema::bind_engine(&conn, engine_id).await
    }

    /// Read one launch identity after the engine is bound.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] for a legacy row or an unbound engine.
    pub async fn launch_identity(&self, id: i64) -> Result<LaunchIdentity, HostError> {
        let conn = self.connection().await?;
        let mut rows = conn
            .query(
                "SELECT launch_id FROM intents WHERE id = ?1 AND kind = 'launch'",
                [id],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let row = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?;
        let launch_id: Option<String> = row.get(0).map_err(|_| HostError::Journal)?;
        let launch_id = launch_id.ok_or(HostError::Journal)?;
        let instance_id = journal_schema::instance_id(&conn).await?;
        let engine_id = journal_schema::engine_id(&conn).await?;
        LaunchIdentity::new(&instance_id, id, &launch_id, &engine_id)
    }

    /// Bind both immutable Docker ids. Rebinding to another id fails.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when either id conflicts or the row is missing.
    pub async fn bind_pair(
        &self,
        id: i64,
        runner_id: &str,
        dind_id: &str,
    ) -> Result<(), HostError> {
        if token_rejected(runner_id) || token_rejected(dind_id) {
            return Err(HostError::Journal);
        }
        let conn = self.connection().await?;
        let changed = conn
            .execute(
                "UPDATE intents SET docker_id = COALESCE(docker_id, ?1), dind_id = COALESCE(dind_id, ?2) WHERE id = ?3 AND (docker_id IS NULL OR docker_id = ?1) AND (dind_id IS NULL OR dind_id = ?2)",
                (runner_id, dind_id, id),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        one_row(changed)
    }

    /// Bind one immutable seed generation to a launch.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when a different generation is already bound.
    pub async fn bind_seed_generation(
        &self,
        id: i64,
        generation_id: &str,
    ) -> Result<(), HostError> {
        if token_rejected(generation_id) {
            return Err(HostError::Journal);
        }
        let conn = self.connection().await?;
        let changed = conn
            .execute(
                "UPDATE intents SET seed_generation_id = COALESCE(seed_generation_id, ?1) WHERE id = ?2 AND kind = 'launch' AND (seed_generation_id IS NULL OR seed_generation_id = ?1)",
                (generation_id, id),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        one_row(changed)
    }

    /// Count all launch reservations that lack proven cleanup.
    ///
    /// Pending, uncertain, stopped, and ID-less rows remain occupied. A worker
    /// exit or queue acknowledgement does not release this count.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the count cannot be read.
    pub async fn occupied_launches(&self) -> Result<u32, HostError> {
        let conn = self.connection().await?;
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
                "SELECT id, kind, subject, state, docker_id, dind_id, github_runner_id, cleanup_proven, launch_id, assignment_key, seed_generation_id FROM intents ORDER BY id",
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
        journal_schema::bootstrap(&conn).await
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
