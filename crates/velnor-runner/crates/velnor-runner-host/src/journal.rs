//! Local turso journal. Each step opens its own connection and commits
//! before the caller runs an external effect.

use std::path::{Path, PathBuf};

use crate::error::HostError;

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
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] on I/O failure or a rejected kind.
    pub async fn begin(&self, kind: &str) -> Result<i64, HostError> {
        if kind_rejected(kind) {
            return Err(HostError::Journal);
        }
        let conn = self.connection().await?;
        conn.execute(
            "INSERT INTO intents (kind, state) VALUES (?1, 'pending')",
            (kind.to_owned(),),
        )
        .await
        .map_err(|_| HostError::Journal)?;
        let id = conn.last_insert_rowid();
        Ok(id)
    }

    /// Record the outcome in a new connection.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] on I/O failure.
    pub async fn finish(&self, id: i64, outcome: Outcome) -> Result<(), HostError> {
        let state = match outcome {
            Outcome::Done => IntentState::Done,
            Outcome::Uncertain => IntentState::Uncertain,
            Outcome::DefiniteFailure => IntentState::Failed,
        };
        let conn = self.connection().await?;
        conn.execute(
            "UPDATE intents SET state = ?1 WHERE id = ?2",
            (state.as_str().to_owned(), id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
        Ok(())
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

    async fn bootstrap(&self) -> Result<(), HostError> {
        let conn = self.connection().await?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS intents (id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, state TEXT NOT NULL)",
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

fn kind_rejected(kind: &str) -> bool {
    kind.is_empty() || kind.chars().any(|ch| matches!(ch, '\'' | '"'))
}
