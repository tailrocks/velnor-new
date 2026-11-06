// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{ops::AsyncFnOnce, path::Path};

use crate::error::HostError;

use super::{file_identity::JournalFile, one_row, same_ids, schema, token_rejected, transaction};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntentState {
    Pending,

    Done,

    Uncertain,

    Failed,
}

impl IntentState {
    pub(super) fn as_str(self) -> &'static str {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Done,

    Uncertain,

    DefiniteFailure,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Journal {
    file: JournalFile,
}

impl Journal {
    pub async fn open(path: &Path) -> Result<Self, HostError> {
        let journal = Self {
            file: JournalFile::open(path).await?,
        };
        journal.bootstrap().await?;
        journal.file.reset_after_bootstrap()?;
        Ok(journal)
    }

    pub async fn begin(&self, kind: &str, subject: &str) -> Result<i64, HostError> {
        if token_rejected(kind) || token_rejected(subject) {
            return Err(HostError::Journal);
        }
        let conn = self.connection().await?;
        commit_live(&conn, kind, subject).await
    }

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

    pub(crate) async fn connection(&self) -> Result<turso::Connection, HostError> {
        self.file.connection().await
    }

    pub(crate) fn path(&self) -> &Path {
        self.file.path()
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

pub(super) async fn live_id(
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
