//! Atomic preparation of identities needed before launch effects.

use super::{Journal, live_id, token_rejected};
use crate::error::HostError;

/// Nonsecret identity committed before acquire, JIT, or Docker is requested.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LaunchIdentity {
    pub(crate) scale_set_id: i64,
    pub(crate) request_id: Option<i64>,
    pub(crate) runner_name: String,
    pub(crate) worker_volume: String,
    pub(crate) docker_engine_id: String,
}

impl Journal {
    /// Begin a launch with its stable resource identity in the same transaction.
    ///
    /// An existing live row is returned unchanged; callers must not use a newly
    /// generated identity to replace the identity of an unresolved launch.
    pub(crate) async fn begin_prepared_launch(
        &self,
        subject: &str,
        identity: &LaunchIdentity,
    ) -> Result<(i64, bool), HostError> {
        if token_rejected(subject) || !identity.valid() {
            return Err(HostError::Journal);
        }
        let conn = self.connection().await?;
        conn.execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = insert_prepared(&conn, subject, identity).await;
        let ended = if result.is_ok() {
            conn.execute("COMMIT", ()).await
        } else {
            conn.execute("ROLLBACK", ()).await
        };
        ended.map_err(|_| HostError::Journal)?;
        result
    }
}

impl LaunchIdentity {
    fn valid(&self) -> bool {
        self.scale_set_id > 0
            && self.request_id.is_none_or(|id| id > 0)
            && [
                self.runner_name.as_str(),
                self.worker_volume.as_str(),
                self.docker_engine_id.as_str(),
            ]
            .into_iter()
            .all(safe_label)
    }
}

fn safe_label(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && !value
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, '\'' | '"'))
}

async fn insert_prepared(
    conn: &turso::Connection,
    subject: &str,
    identity: &LaunchIdentity,
) -> Result<(i64, bool), HostError> {
    if let Some(id) = live_id(conn, "launch", subject).await? {
        return Ok((id, false));
    }
    conn.execute(
        "INSERT INTO intents (kind, subject, state, worker_volume, scale_set_id, runner_request_id, runner_name, docker_engine_id, launch_phase) VALUES ('launch', ?1, 'pending', ?2, ?3, ?4, ?5, ?6, 'prepared')",
        (
            subject.to_owned(),
            identity.worker_volume.clone(),
            identity.scale_set_id,
            identity.request_id,
            identity.runner_name.clone(),
            identity.docker_engine_id.clone(),
        ),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    Ok((conn.last_insert_rowid(), true))
}
