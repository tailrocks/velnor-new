//! Read-only reservation state for replay and recovery decisions.

use crate::error::HostError;

use super::super::storage::read_transaction;
use super::super::{AssignmentIdentity, ConnectionJournal};

/// Durable capacity state of one assignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReservationState {
    /// Charge is active and its launch row may be revalidated before effects.
    Reserved,
    /// A remote or local effect may have happened.
    Uncertain,
    /// A recovery worker currently owns reconciliation.
    Recovering,
    /// Both remote and local cleanup proofs released the charge.
    Released,
}

/// Immutable charge plus the current exact effect boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReservationSnapshot {
    /// Durable reservation identity.
    pub(crate) reservation_id: i64,
    /// Current durable charge state.
    pub(crate) state: ReservationState,
    /// Generation consumed for initial admission.
    pub(crate) initial_sample_generation_id: Option<String>,
    /// Whether no external launch effect has been claimed yet.
    pub(crate) resume_eligible: bool,
    /// CPU charge remains fixed for the life of the reservation.
    pub(crate) cpu_millicores: u64,
    /// Memory charge remains fixed for the life of the reservation.
    pub(crate) memory_bytes: u64,
}

impl ConnectionJournal {
    /// Read one assignment's scoped reservation without changing charge or permit state.
    pub(crate) async fn reservation(
        &self,
        identity: &AssignmentIdentity,
    ) -> Result<Option<ReservationSnapshot>, HostError> {
        read_transaction(&self.journal, async |connection| {
            self.validate_assignment(connection, identity).await?;
            let assignment_key = identity.storage_key();
            let mut rows = connection
                .query(
                    "SELECT r.id, r.state, r.binding_digest, r.sample_generation_id, r.cpu_millicores, r.memory_bytes, i.state, i.cleanup_proven, i.acquire_attempted, i.jit_requested, i.docker_id, i.dind_id, i.worker_volume, i.launch_phase FROM capacity_reservations AS r JOIN connection_intents AS i ON i.id = r.intent_id WHERE r.connection_id = ?1 AND r.assignment_key = ?2 AND i.connection_id = ?1",
                    (self.connection_id().as_str(), assignment_key.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
                return Ok(None);
            };
            if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
                return Err(HostError::Journal);
            }
            let binding_digest: String = row.get(2).map_err(|_| HostError::Journal)?;
            if binding_digest != self.binding_digest {
                return Err(HostError::Journal);
            }
            let state = parse_state(&row.get::<String>(1).map_err(|_| HostError::Journal)?)?;
            let launch_state: String = row.get(6).map_err(|_| HostError::Journal)?;
            let cleaned: bool = row.get(7).map_err(|_| HostError::Journal)?;
            let acquire_attempted: bool = row.get(8).map_err(|_| HostError::Journal)?;
            let jit_requested: bool = row.get(9).map_err(|_| HostError::Journal)?;
            let docker_id: Option<String> = row.get(10).map_err(|_| HostError::Journal)?;
            let dind_id: Option<String> = row.get(11).map_err(|_| HostError::Journal)?;
            let volume: Option<String> = row.get(12).map_err(|_| HostError::Journal)?;
            let phase: Option<String> = row.get(13).map_err(|_| HostError::Journal)?;
            let cpu = u64::try_from(row.get::<i64>(4).map_err(|_| HostError::Journal)?)
                .map_err(|_| HostError::Journal)?;
            let memory = u64::try_from(row.get::<i64>(5).map_err(|_| HostError::Journal)?)
                .map_err(|_| HostError::Journal)?;
            Ok(Some(ReservationSnapshot {
                reservation_id: row.get(0).map_err(|_| HostError::Journal)?,
                state,
                initial_sample_generation_id: row
                    .get(3)
                    .map_err(|_| HostError::Journal)?,
                resume_eligible: state == ReservationState::Reserved
                    && launch_state == "pending"
                    && !cleaned
                    && !acquire_attempted
                    && !jit_requested
                    && docker_id.is_none()
                    && dind_id.is_none()
                    && volume.is_none()
                    && phase.is_none(),
                cpu_millicores: cpu,
                memory_bytes: memory,
            }))
        })
        .await
    }
}

fn parse_state(value: &str) -> Result<ReservationState, HostError> {
    match value {
        "reserved" => Ok(ReservationState::Reserved),
        "uncertain" => Ok(ReservationState::Uncertain),
        "recovering" => Ok(ReservationState::Recovering),
        "released" => Ok(ReservationState::Released),
        _ => Err(HostError::Journal),
    }
}
