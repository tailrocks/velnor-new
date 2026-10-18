//! Atomic reservation admission before any remote or Docker effect.

use crate::error::HostError;
use crate::launch::StartPermit;

use super::super::storage::{has_ambiguous_legacy_work, in_transaction};
use super::super::{AssignmentIdentity, AssignmentKey, ConnectionJournal};
use super::storage::{has_unreserved_scoped_launch, reservation_for_assignment, validate_request};
use super::storage_budget::StorageAdmission;

/// Resources charged by one assignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReservationRequest<'a> {
    /// Typed connection and Scale Set assignment identity.
    pub(crate) assignment: &'a AssignmentIdentity,
    /// Selected Docker engine identity.
    pub(crate) engine_id: &'a str,
    /// Digest of the selected Docker root.
    pub(crate) docker_root_digest: &'a str,
    /// Total runner and private `DinD` CPU charge.
    pub(crate) cpu_millicores: u64,
    /// Total runner and private `DinD` memory charge.
    pub(crate) memory_bytes: u64,
    /// Resource sample generation that authorized this charge.
    pub(crate) sample_generation_id: &'a str,
}

/// Result of a durable reservation attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReservationResult {
    /// A live reservation exists.
    Reserved {
        /// Durable reservation identity.
        reservation_id: i64,
    },
    /// Existing capacity was revalidated with a fresh, single-use sample.
    Resumed {
        /// Durable reservation identity.
        reservation_id: i64,
    },
    /// Existing effects or uncertainty require reconciliation before resuming.
    ReconcileRequired {
        /// Durable reservation identity.
        reservation_id: i64,
    },
    /// This assignment already completed exact cleanup.
    Released {
        /// Durable reservation identity.
        reservation_id: i64,
    },
    /// The shared host budget cannot accept this charge.
    AtCapacity,
}

impl ConnectionJournal {
    /// Reserve host capacity before remote or Docker work starts.
    ///
    /// The transaction checks replay, capacity, and the one-use permit together.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when identity, permit, or storage checks fail.
    pub(crate) async fn try_reserve(
        &self,
        request: ReservationRequest<'_>,
        permit: StartPermit,
    ) -> Result<ReservationResult, HostError> {
        self.try_reserve_inner(request, permit, None).await
    }

    /// Admit a reservation with a fresh storage plan and filesystem sample.
    pub(crate) async fn try_reserve_with_storage(
        &self,
        request: ReservationRequest<'_>,
        permit: StartPermit,
        storage: StorageAdmission,
    ) -> Result<ReservationResult, HostError> {
        self.try_reserve_inner(request, permit, Some(storage)).await
    }

    async fn try_reserve_inner(
        &self,
        request: ReservationRequest<'_>,
        permit: StartPermit,
        storage: Option<StorageAdmission>,
    ) -> Result<ReservationResult, HostError> {
        validate_request(request)?;
        let cpu = i64::try_from(request.cpu_millicores).map_err(|_| HostError::Journal)?;
        let memory = i64::try_from(request.memory_bytes).map_err(|_| HostError::Journal)?;
        let assignment_key = request.assignment.storage_key();
        in_transaction(&self.journal, async move |connection| {
            self.validate_scope(connection).await?;
            validate_bound_identity(connection, self, request.assignment).await?;
            if has_ambiguous_legacy_work(connection).await?
                || has_unreserved_scoped_launch(connection).await?
            {
                return Err(HostError::Journal);
            }
            if let Some(existing) = reservation_for_assignment(
                connection,
                self.connection_id.as_str(),
                request.assignment,
            )
            .await?
            {
                if existing.binding_digest != self.binding_digest
                    || existing.engine_id != request.engine_id
                    || existing.docker_root_digest != request.docker_root_digest
                    || existing.cpu_millicores != cpu
                    || existing.memory_bytes != memory
                {
                    return Err(HostError::Journal);
                }
                if existing.released {
                    return Ok(ReservationResult::Released {
                        reservation_id: existing.id,
                    });
                }
                if existing.state != "reserved"
                    || !launch_is_pristine(connection, self.connection_id.as_str(), existing.intent_id).await?
                {
                    return Ok(ReservationResult::ReconcileRequired {
                        reservation_id: existing.id,
                    });
                }
                let Some(storage) = storage.as_ref() else {
                    return Ok(ReservationResult::ReconcileRequired {
                        reservation_id: existing.id,
                    });
                };
                if !super::storage_budget::admission_matches_existing(
                    connection,
                    existing.id,
                    storage,
                )
                .await?
                {
                    return Ok(ReservationResult::ReconcileRequired {
                        reservation_id: existing.id,
                    });
                }
                if !super::storage_budget::admission_fits(
                    connection,
                    storage,
                    request.sample_generation_id,
                    request.engine_id,
                    request.docker_root_digest,
                    request.memory_bytes,
                    Some(existing.id),
                )
                .await?
                {
                    return Ok(ReservationResult::AtCapacity);
                }
                validate_permit(request, &permit)?;
                ensure_sample_is_new(connection, request.sample_generation_id).await?;
                if !permit.consume_for_admission(request.engine_id, request.docker_root_digest) {
                    return Err(HostError::Journal);
                }
                record_sample_use(connection, existing.id, request.sample_generation_id, "resume").await?;
                let profile = super::storage_budget::current_profile(connection)
                    .await?
                    .ok_or(HostError::Journal)?;
                super::storage_budget::append_storage_sample(
                    connection,
                    existing.id,
                    &profile,
                    storage.sample(),
                )
                .await?;
                return Ok(ReservationResult::Resumed {
                    reservation_id: existing.id,
                });
            }
            let Some(storage) = storage.as_ref() else {
                return Ok(ReservationResult::AtCapacity);
            };
            let Some(profile) = super::storage_budget::current_profile(connection).await? else {
                return Ok(ReservationResult::AtCapacity);
            };
            if !super::storage_budget::admission_fits(
                connection,
                storage,
                request.sample_generation_id,
                request.engine_id,
                request.docker_root_digest,
                request.memory_bytes,
                None,
            )
            .await?
            {
                return Ok(ReservationResult::AtCapacity);
            }
            if !capacity_available(
                connection,
                request,
                storage.plan().retained_memory_bytes()?,
            )
            .await?
            {
                return Ok(ReservationResult::AtCapacity);
            }
            validate_permit(request, &permit)?;
            let scale_set_id = request.assignment.scale_set_id();
            let key = assignment_key;
            let (request_id, runner_name) = match request.assignment.key() {
                AssignmentKey::AssignedRequest { request_id } => {
                    (Some(*request_id), Some(format!("v{request_id}")))
                }
                AssignmentKey::PopulationMessage { message_id } => {
                    (None, Some(format!("m{message_id}")))
                }
                AssignmentKey::Session { name } => (None, Some(name.clone())),
                AssignmentKey::Remote { .. } => (None, None),
            };
            if !permit.consume_for_admission(request.engine_id, request.docker_root_digest) {
                return Err(HostError::Journal);
            }
            connection
                .execute(
                    "INSERT INTO connection_intents (connection_id, kind, subject, state, scale_set_id, runner_request_id, runner_name, assignment_key) VALUES (?1, 'launch', ?2, 'pending', ?3, ?4, ?5, ?2)",
                    (self.connection_id.as_str(), key.as_str(), scale_set_id, request_id, runner_name),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let intent_id = connection.last_insert_rowid();
            connection
                .execute(
                    "INSERT INTO capacity_reservations (intent_id, connection_id, assignment_key, engine_id, docker_root_digest, binding_digest, sample_generation_id, cpu_millicores, memory_bytes, state) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'reserved')",
                    (
                        intent_id,
                        self.connection_id.as_str(),
                        key,
                        request.engine_id,
                        request.docker_root_digest,
                        self.binding_digest.as_str(),
                        request.sample_generation_id,
                        cpu,
                        memory,
                    ),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let reservation_id = connection.last_insert_rowid();
            record_sample_use(connection, reservation_id, request.sample_generation_id, "initial").await?;
            super::storage_budget::insert_reservation_storage(
                connection,
                reservation_id,
                &profile,
                storage,
            )
            .await?;
            Ok(ReservationResult::Reserved {
                reservation_id,
            })
        })
        .await
    }
}

fn validate_permit(request: ReservationRequest<'_>, permit: &StartPermit) -> Result<(), HostError> {
    if permit.engine_id() != request.engine_id
        || permit.docker_root_digest() != request.docker_root_digest
        || permit.charge() != (request.cpu_millicores, request.memory_bytes)
        || permit.sample_generation_id() != request.sample_generation_id
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn ensure_sample_is_new(
    connection: &turso::Connection,
    generation_id: &str,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT 1 FROM capacity_reservation_samples WHERE sample_generation_id = ?1 LIMIT 1",
            [generation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        Err(HostError::Journal)
    } else {
        Ok(())
    }
}

async fn record_sample_use(
    connection: &turso::Connection,
    reservation_id: i64,
    generation_id: &str,
    purpose: &str,
) -> Result<(), HostError> {
    connection
        .execute(
            "INSERT INTO capacity_reservation_samples (reservation_id, sample_generation_id, purpose) VALUES (?1, ?2, ?3)",
            (reservation_id, generation_id, purpose),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn launch_is_pristine(
    connection: &turso::Connection,
    connection_id: &str,
    intent_id: i64,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT state, cleanup_proven, acquire_attempted, jit_requested, docker_id, dind_id, worker_volume, launch_phase FROM connection_intents WHERE id = ?1 AND connection_id = ?2 AND kind = 'launch'",
            (intent_id, connection_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(false);
    };
    let state: String = row.get(0).map_err(|_| HostError::Journal)?;
    let cleaned: bool = row.get(1).map_err(|_| HostError::Journal)?;
    let acquire_attempted: bool = row.get(2).map_err(|_| HostError::Journal)?;
    let jit_requested: bool = row.get(3).map_err(|_| HostError::Journal)?;
    let docker_id: Option<String> = row.get(4).map_err(|_| HostError::Journal)?;
    let dind_id: Option<String> = row.get(5).map_err(|_| HostError::Journal)?;
    let volume: Option<String> = row.get(6).map_err(|_| HostError::Journal)?;
    let phase: Option<String> = row.get(7).map_err(|_| HostError::Journal)?;
    Ok(state == "pending"
        && !cleaned
        && !acquire_attempted
        && !jit_requested
        && docker_id.is_none()
        && dind_id.is_none()
        && volume.is_none()
        && phase.is_none())
}

async fn validate_bound_identity(
    connection: &turso::Connection,
    scope: &ConnectionJournal,
    identity: &AssignmentIdentity,
) -> Result<(), HostError> {
    if identity.connection_id() != scope.connection_id() {
        return Err(HostError::Journal);
    }
    let mut rows = connection
        .query(
            "SELECT scale_set_id FROM connection_runtime_bindings WHERE connection_id = ?1",
            [scope.connection_id().as_str()],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let id: Option<i64> = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    if id == Some(identity.scale_set_id()) {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

async fn capacity_available(
    connection: &turso::Connection,
    request: ReservationRequest<'_>,
    storage_memory_bytes: u64,
) -> Result<bool, HostError> {
    let limits = super::storage::capacity_limits(connection).await?;
    let usage = super::storage::capacity_usage(connection).await?;
    let requested_memory = request
        .memory_bytes
        .checked_add(storage_memory_bytes)
        .ok_or(HostError::Journal)?;
    Ok(usage.jobs < limits.jobs
        && usage
            .cpu_millicores
            .checked_add(request.cpu_millicores)
            .is_some_and(|sum| sum <= limits.cpu_millicores)
        && usage
            .memory_bytes
            .checked_add(requested_memory)
            .is_some_and(|sum| sum <= limits.memory_bytes))
}
