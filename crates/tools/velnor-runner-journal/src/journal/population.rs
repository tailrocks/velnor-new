//! Historical, non-authorizing Scale Set population observations.

use std::fmt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use velnor_runner_github::{PopulationObservationSource, SessionPopulationObservation, Statistics};

use crate::error::HostError;
use crate::journal::{Journal, ScaleSetSessionIdentity};

mod store;
use store::record_snapshot;

/// Source response for one persisted population snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaleSetPopulationSource {
    /// Statistics returned when the exact queue session was created.
    SessionCreated,
    /// Statistics returned with one exact poll message.
    PollBatch,
}

impl ScaleSetPopulationSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::SessionCreated => "session_created",
            Self::PollBatch => "poll_batch",
        }
    }

    fn parse(value: &str) -> Result<Self, HostError> {
        match value {
            "session_created" => Ok(Self::SessionCreated),
            "poll_batch" => Ok(Self::PollBatch),
            _ => Err(HostError::Journal),
        }
    }
}

/// Durable historical data from one exact session response.
///
/// This record is useful for audit and restart diagnostics only. It is not a
/// live queue observation, does not mint an assigned-demand permit, and must
/// never be used to authorize JIT after process restart.
#[derive(Clone, PartialEq, Eq)]
pub struct ScaleSetPopulationSnapshot {
    intent_id: i64,
    session_id: String,
    scale_set_id: i64,
    source: ScaleSetPopulationSource,
    message_id: Option<i64>,
    observed_at: SystemTime,
    statistics: Statistics,
}

impl fmt::Debug for ScaleSetPopulationSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ScaleSetPopulationSnapshot")
            .field("intent_id", &self.intent_id)
            .field("session_id", &"[redacted]")
            .field("scale_set_id", &self.scale_set_id)
            .field("source", &self.source)
            .field("message_id", &self.message_id)
            .field("observed_at", &self.observed_at)
            .field("statistics", &self.statistics)
            .finish()
    }
}

impl ScaleSetPopulationSnapshot {
    /// Copy one live, exact-session observation into journal-safe historical
    /// data. This conversion does not preserve any queue or admission authority.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] for invalid identity, source metadata, or
    /// a timestamp outside the journal's millisecond range.
    pub fn from_observation(
        intent_id: i64,
        observation: &SessionPopulationObservation,
    ) -> Result<Self, HostError> {
        let observed_at = observation.observed_at();
        let millis = observed_at
            .duration_since(UNIX_EPOCH)
            .map_err(|_| HostError::Journal)?
            .as_millis();
        i64::try_from(millis).map_err(|_| HostError::Journal)?;
        let source = match observation.source() {
            PopulationObservationSource::SessionCreated => ScaleSetPopulationSource::SessionCreated,
            PopulationObservationSource::PollBatch => ScaleSetPopulationSource::PollBatch,
        };
        let snapshot = Self {
            intent_id,
            session_id: observation.session_id().to_owned(),
            scale_set_id: observation.scale_set_id(),
            source,
            message_id: observation.message_id(),
            observed_at,
            statistics: observation.statistics().clone(),
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    #[cfg(test)]
    pub(super) fn test_only(
        intent_id: i64,
        session_id: &str,
        scale_set_id: i64,
        source: ScaleSetPopulationSource,
        message_id: Option<i64>,
        observed_at: SystemTime,
        statistics: Statistics,
    ) -> Result<Self, HostError> {
        let snapshot = Self {
            intent_id,
            session_id: session_id.to_owned(),
            scale_set_id,
            source,
            message_id,
            observed_at,
            statistics,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    /// Durable controller-session intent this observation belongs to.
    #[must_use]
    pub const fn intent_id(&self) -> i64 {
        self.intent_id
    }

    /// Opaque exact queue-session ID. Debug formatting redacts this value.
    #[must_use]
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Exact Actions Service Scale Set ID.
    #[must_use]
    pub const fn scale_set_id(&self) -> i64 {
        self.scale_set_id
    }

    /// Protocol response that supplied these statistics.
    #[must_use]
    pub const fn source(&self) -> ScaleSetPopulationSource {
        self.source
    }

    /// Queue message ID for a poll response; absent for create-time statistics.
    #[must_use]
    pub const fn message_id(&self) -> Option<i64> {
        self.message_id
    }

    /// Local receipt time persisted with the snapshot.
    #[must_use]
    pub const fn observed_at(&self) -> SystemTime {
        self.observed_at
    }

    /// Raw counts as returned by the pinned service protocol.
    #[must_use]
    pub const fn statistics(&self) -> &Statistics {
        &self.statistics
    }

    fn validate(&self) -> Result<(), HostError> {
        let millis = self
            .observed_at
            .duration_since(UNIX_EPOCH)
            .map_err(|_| HostError::Journal)?
            .as_millis();
        if self.intent_id <= 0
            || self.session_id.is_empty()
            || self.session_id.len() > 256
            || self.session_id.chars().any(char::is_whitespace)
            || self.scale_set_id <= 0
            || i64::try_from(millis).is_err()
            || match (self.source, self.message_id) {
                (ScaleSetPopulationSource::SessionCreated, None) => false,
                (ScaleSetPopulationSource::PollBatch, Some(id)) => id < 0,
                _ => true,
            }
        {
            return Err(HostError::Journal);
        }
        Ok(())
    }

    fn same_payload(&self, other: &Self) -> bool {
        self.intent_id == other.intent_id
            && self.session_id == other.session_id
            && self.scale_set_id == other.scale_set_id
            && self.source == other.source
            && self.message_id == other.message_id
            && self.statistics == other.statistics
    }

    fn observed_at_ms(&self) -> Result<i64, HostError> {
        let millis = self
            .observed_at
            .duration_since(UNIX_EPOCH)
            .map_err(|_| HostError::Journal)?
            .as_millis();
        i64::try_from(millis).map_err(|_| HostError::Journal)
    }
}

/// Result of preserving one observation without changing any worker-slot row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopulationSnapshotWrite {
    /// A first snapshot or newer message replaced the previous snapshot.
    Stored,
    /// Exact create or message redelivery was already recorded.
    Unchanged,
    /// An older message was ignored; the latest same-session snapshot remains.
    OlderMessageIgnored,
}

impl Journal {
    /// Persist a fresh response snapshot before processing its queue effects.
    ///
    /// The write is bound to the exact open journaled session and Scale Set.
    /// Create-time data is first-only; poll message IDs are monotonic only
    /// within this session. Equal-message redelivery is idempotent only when
    /// counts agree. This operation never changes launch reservations.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] for identity mismatch, conflicting
    /// redelivery, invalid ordering, closed/uncertain session state, or storage
    /// failure. The caller must hold the message and perform no queue effects.
    pub async fn record_scale_set_population_snapshot(
        &self,
        identity: &ScaleSetSessionIdentity,
        snapshot: &ScaleSetPopulationSnapshot,
    ) -> Result<PopulationSnapshotWrite, HostError> {
        snapshot.validate()?;
        if self.read_only || identity.scale_set_id() != snapshot.scale_set_id {
            return Err(HostError::Journal);
        }
        let conn = self.connection().await?;
        conn.execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = record_snapshot(&conn, identity, snapshot).await;
        let end = if result.is_ok() {
            conn.execute("COMMIT", ()).await
        } else {
            conn.execute("ROLLBACK", ()).await
        };
        end.map_err(|_| HostError::Journal)?;
        result
    }

    /// Read the latest stored response for audit/restart diagnostics only.
    ///
    /// A returned value is historical after reopen and cannot authorize pool
    /// admission, assigned demand, Acquire, JIT, or worker start.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] for a malformed row or database failure.
    pub async fn scale_set_population_snapshot(
        &self,
        intent_id: i64,
    ) -> Result<Option<ScaleSetPopulationSnapshot>, HostError> {
        if intent_id <= 0 {
            return Err(HostError::Journal);
        }
        let conn = self.connection().await?;
        let mut rows = conn
            .query(
                "SELECT p.session_id, p.scale_set_id, p.source, p.message_id, p.observed_at_ms, p.total_available_jobs, p.total_acquired_jobs, p.total_assigned_jobs, p.total_running_jobs, p.total_registered_runners, p.total_busy_runners, p.total_idle_runners FROM scale_set_population_observations AS p JOIN scale_set_sessions AS s ON s.intent_id = p.intent_id JOIN intents AS i ON i.id = p.intent_id WHERE p.intent_id = ?1 AND i.kind = 'scale-set-session'",
                [intent_id],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
            return Ok(None);
        };
        let session_id: String = row.get(0).map_err(|_| HostError::Journal)?;
        let scale_set_id: i64 = row.get(1).map_err(|_| HostError::Journal)?;
        let source = ScaleSetPopulationSource::parse(
            &row.get::<String>(2).map_err(|_| HostError::Journal)?,
        )?;
        let message_id: Option<i64> = row.get(3).map_err(|_| HostError::Journal)?;
        let observed_at_ms: i64 = row.get(4).map_err(|_| HostError::Journal)?;
        let statistics = Statistics {
            total_available_jobs: row.get(5).map_err(|_| HostError::Journal)?,
            total_acquired_jobs: row.get(6).map_err(|_| HostError::Journal)?,
            total_assigned_jobs: row.get(7).map_err(|_| HostError::Journal)?,
            total_running_jobs: row.get(8).map_err(|_| HostError::Journal)?,
            total_registered_runners: row.get(9).map_err(|_| HostError::Journal)?,
            total_busy_runners: row.get(10).map_err(|_| HostError::Journal)?,
            total_idle_runners: row.get(11).map_err(|_| HostError::Journal)?,
        };
        let snapshot = ScaleSetPopulationSnapshot {
            intent_id,
            session_id,
            scale_set_id,
            source,
            message_id,
            observed_at: UNIX_EPOCH
                .checked_add(Duration::from_millis(
                    u64::try_from(observed_at_ms).map_err(|_| HostError::Journal)?,
                ))
                .ok_or(HostError::Journal)?,
            statistics,
        };
        snapshot.validate()?;
        if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
            return Err(HostError::Journal);
        }
        Ok(Some(snapshot))
    }
}
