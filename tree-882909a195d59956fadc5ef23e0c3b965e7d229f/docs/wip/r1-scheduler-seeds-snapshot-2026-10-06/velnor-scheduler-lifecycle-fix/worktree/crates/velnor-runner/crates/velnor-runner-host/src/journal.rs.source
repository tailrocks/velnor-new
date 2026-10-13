//! Local turso journal. Each step opens its own connection and commits
//! before the caller runs an external effect.

use std::ops::AsyncFnOnce;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::daemon_lock::canonical_journal_path;
use crate::error::HostError;
use crate::journal_assignment::{commit_assignment, commit_launch};
use crate::journal_sql::{commit_live, intent_row, one_row, token_rejected};
use crate::reconcile::IntentRow;
use process::{JournalProcessState, process_state};

pub use crate::launch_identity::LaunchIdentity;

mod process;

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

/// Outcome of an atomic capacity reservation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LaunchReservation {
    /// No slot was free. No queue or Docker effect may run.
    AtCapacity,
    /// This launch already has a durable identity.
    Existing(i64),
    /// This call created a durable identity before external effects.
    New(i64),
    /// This assignment already completed and must not be acquired again.
    Completed(i64),
}

/// File-backed journal.
#[derive(Debug, Clone)]
pub struct Journal {
    path: PathBuf,
    process: Arc<JournalProcessState>,
}

impl Journal {
    /// Create the file and schema.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Path`] for a noncanonical path or
    /// [`HostError::Journal`] when turso cannot open the path.
    pub async fn open(path: &Path) -> Result<Self, HostError> {
        let path = canonical_journal_path(path)?;
        let journal = Self {
            process: process_state(&path)?,
            path,
        };
        let _write = journal.process.write_lock.lock().await;
        journal.bootstrap().await?;
        drop(_write);
        Ok(journal)
    }

    #[cfg(test)]
    pub(crate) fn shares_process_state(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.process, &other.process)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
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
        let _write = self.write_guard().await;
        self.sync_lineage().await?;
        let conn = self.connection().await?;
        let result = commit_live(&conn, kind, subject).await.map(|(id, _)| id);
        self.sync_after(result).await
    }

    /// Reserve one acquired scale-set request before acquire or JIT.
    ///
    /// `runnerRequestId` is stable across queue redelivery. `message_id` is
    /// used only to adopt rows written by the old message-based key.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the request id is ambiguous or the write fails.
    pub(crate) async fn reserve_assignment(
        &self,
        set_id: i64,
        request_id: i64,
        message_id: i64,
        capacity: u32,
    ) -> Result<LaunchReservation, HostError> {
        if set_id <= 0 || request_id < 0 || message_id < 0 || capacity == 0 {
            return Err(HostError::Journal);
        }
        let _write = self.write_guard().await;
        self.sync_lineage().await?;
        let conn = self.connection().await?;
        let result = commit_assignment(&conn, set_id, request_id, message_id, capacity).await;
        self.sync_after(result).await
    }

    /// Reserve one unassigned scale runner before JIT or Docker.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the reservation cannot be stored.
    pub(crate) async fn reserve_launch(
        &self,
        subject: &str,
        capacity: u32,
    ) -> Result<LaunchReservation, HostError> {
        if token_rejected(subject) || capacity == 0 {
            return Err(HostError::Journal);
        }
        let _write = self.write_guard().await;
        self.sync_lineage().await?;
        let conn = self.connection().await?;
        let result = commit_launch(&conn, subject, capacity).await;
        self.sync_after(result).await
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
        let _write = self.write_guard().await;
        self.sync_lineage().await?;
        let conn = self.connection().await?;
        let changed = conn
            .execute(
                "UPDATE intents SET state = ?1 WHERE id = ?2 AND cleanup_proven = 0 AND (state IN ('pending', 'uncertain') OR state = ?1)",
                (state.as_str().to_owned(), id),
            )
            .await
            .map_err(|_| HostError::Journal);
        self.sync_after(changed.and_then(one_row)).await
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

    /// Record that cleanup of this row's ids is proven.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the row is missing.
    pub async fn record_cleanup(&self, id: i64) -> Result<(), HostError> {
        let _write = self.write_guard().await;
        self.sync_lineage().await?;
        let conn = self.connection().await?;
        conn.execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = async {
            let changed = conn
                .execute("UPDATE intents SET cleanup_proven = 1 WHERE id = ?1", [id])
                .await
                .map_err(|_| HostError::Journal)?;
            one_row(changed)?;
            conn.execute("DELETE FROM completion_cleanup WHERE intent_id = ?1", [id])
                .await
                .map_err(|_| HostError::Journal)?;
            Ok(())
        }
        .await;
        let ended = if result.is_ok() {
            conn.execute("COMMIT", ()).await
        } else {
            conn.execute("ROLLBACK", ()).await
        };
        ended.map_err(|_| HostError::Journal)?;
        self.sync_after(result).await
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
                "SELECT id, kind, subject, state, docker_id, dind_id, github_runner_id, cleanup_proven, launch_id, assignment_key, seed_generation_id, acquire_attempted, acquire_resolved, acquired, jit_requested, runner_completed FROM intents ORDER BY id",
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
        crate::journal_schema::bootstrap(&conn).await
    }

    pub(super) async fn connection(&self) -> Result<turso::Connection, HostError> {
        let text = self.path.to_str().ok_or(HostError::Path)?;
        let db = turso::Builder::new_local(text)
            .build()
            .await
            .map_err(|_| HostError::Journal)?;
        db.connect().map_err(|_| HostError::Journal)
    }
}
