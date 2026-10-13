//! Local turso journal. Each step opens its own connection and commits
//! before the caller runs an external effect.

use std::path::{Path, PathBuf};

use crate::error::HostError;

mod auth_intent;
mod capacity;
mod cleanup;
mod controller;
mod events;
mod intent;
mod launch;
mod lifecycle;
mod schema;
mod worker_volume;

pub use auth_intent::{
    DiscoveryCredentialOutcome, DiscoveryCredentialScope, DiscoveryCredentialStep,
};
pub use capacity::{
    BatchCapacityClaim, BatchOfferClaim, BatchOfferState, CapacityClaim, LaunchEffectState,
    ReplayRoute, ScopedLaunchIdentity,
};
pub use cleanup::{
    CleanupCheckpointIdentity, CleanupChildren, CleanupDiagnostics, CleanupDisposition,
    CleanupStopPolicy, OuterNetworkCleanupState, OuterNetworkRemovalProof, PhysicalCleanupProof,
    PostActionDisposition, RunnerStartObservation,
};
pub use lifecycle::RunnerStartIntent;

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

/// Result of an atomic admission check and launch reservation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchClaim {
    /// A durable launch row was created before any external effect.
    New(i64),
    /// A matching live row already exists; external effects must not be replayed blindly.
    Existing(i64),
    /// A matching launch completed remotely and owned cleanup was proven.
    Resolved(i64),
    /// Drain was committed before this new launch could reserve capacity.
    Draining,
}

/// File-backed journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Journal {
    path: PathBuf,
    read_only: bool,
}

pub(super) async fn outer_network_removal_started(
    conn: &turso::Connection,
    launch_id: i64,
) -> Result<bool, HostError> {
    let mut rows = conn
        .query(
            "SELECT EXISTS (SELECT 1 FROM worker_cleanup_steps WHERE launch_id = ?1 AND step_key = 'outer-network-removal')",
            [launch_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    match row.get::<i64>(0).map_err(|_| HostError::Journal)? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(HostError::Journal),
    }
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
            read_only: false,
        };
        journal.bootstrap().await?;
        Ok(journal)
    }

    async fn bootstrap(&self) -> Result<(), HostError> {
        let conn = self.connection().await?;
        schema::bootstrap(&conn).await
    }

    async fn connection(&self) -> Result<turso::Connection, HostError> {
        let text = self.path.to_str().ok_or(HostError::Path)?;
        let db = turso::Builder::new_local(text)
            .read_only(self.read_only)
            .build()
            .await
            .map_err(|_| HostError::Journal)?;
        db.connect().map_err(|_| HostError::Journal)
    }
}

async fn live_id(
    conn: &turso::Connection,
    kind: &str,
    subject: &str,
) -> Result<Option<i64>, HostError> {
    let query = if kind == "launch" {
        "SELECT id FROM intents WHERE kind = ?1 AND subject = ?2 AND NOT (state = 'failed' AND replay_key_version = 1 AND effect_state = 'definite_no_effect' AND docker_id IS NULL AND github_runner_id IS NULL AND dind_id IS NULL AND worker_volume IS NULL AND observed_job_id IS NULL AND observed_workflow_run_id IS NULL AND remote_terminal = 0) ORDER BY id DESC LIMIT 1"
    } else {
        "SELECT id FROM intents WHERE kind = ?1 AND subject = ?2 AND state != 'failed' AND cleanup_proven = 0 ORDER BY id DESC LIMIT 1"
    };
    let mut rows = conn
        .query(query, (kind.to_owned(), subject.to_owned()))
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let id: i64 = row.get(0).map_err(|_| HostError::Journal)?;
    Ok(Some(id))
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

#[cfg(test)]
mod tests;
