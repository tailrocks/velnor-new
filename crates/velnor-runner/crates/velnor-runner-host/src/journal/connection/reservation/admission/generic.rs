//! Admit reservations only for journal tests.

use crate::error::HostError;
use crate::launch::StartPermit;

use super::super::super::ConnectionJournal;
use super::super::super::storage::{has_ambiguous_legacy_work, in_transaction};
use super::super::storage as reservation_storage;
use super::super::storage::{
    ExistingReservation, has_unreserved_scoped_launch, reservation_for_assignment, validate_request,
};
use super::super::storage_budget;
use super::super::storage_budget::StorageAdmission;
use super::record::{
    capacity_available, insert_capacity_reservation, insert_launch_intent,
    runner_assignment_fields, validate_bound_identity,
};
use super::validate_permit;
use super::{ReservationRequest, ReservationResult, ensure_sample_is_new, record_sample_use};

pub(super) struct ReservationAttempt<'a> {
    pub(super) request: ReservationRequest<'a>,
    pub(super) permit: Option<StartPermit>,
    pub(super) storage: Option<StorageAdmission>,
    pub(super) cpu: i64,
    pub(super) memory: i64,
    pub(super) assignment_key: String,
}

impl ConnectionJournal {
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
        let attempt = ReservationAttempt {
            request,
            permit: Some(permit),
            storage,
            cpu: i64::try_from(request.cpu_millicores).map_err(|_| HostError::Journal)?,
            memory: i64::try_from(request.memory_bytes).map_err(|_| HostError::Journal)?,
            assignment_key: request.assignment.storage_key(),
        };
        in_transaction(&self.journal, async move |connection| {
            reserve_in_transaction(connection, self, attempt).await
        })
        .await
    }
}

async fn reserve_in_transaction(
    connection: &turso::Connection,
    journal: &ConnectionJournal,
    mut attempt: ReservationAttempt<'_>,
) -> Result<ReservationResult, HostError> {
    journal.validate_scope(connection).await?;
    validate_bound_identity(connection, journal, attempt.request.assignment).await?;
    if has_ambiguous_legacy_work(connection).await?
        || has_unreserved_scoped_launch(connection).await?
    {
        return Err(HostError::Journal);
    }
    let existing = reservation_for_assignment(
        connection,
        journal.connection_id.as_str(),
        attempt.request.assignment,
    )
    .await?;
    match existing {
        Some(existing) => resume_reservation(connection, journal, &mut attempt, existing).await,
        None => create_reservation(connection, journal, &mut attempt).await,
    }
}

async fn resume_reservation(
    connection: &turso::Connection,
    journal: &ConnectionJournal,
    attempt: &mut ReservationAttempt<'_>,
    existing: ExistingReservation,
) -> Result<ReservationResult, HostError> {
    let request = attempt.request;
    if existing.binding_digest != journal.binding_digest
        || existing.engine_id != request.engine_id
        || existing.docker_root_digest != request.docker_root_digest
        || existing.cpu_millicores != attempt.cpu
        || existing.memory_bytes != attempt.memory
    {
        return Err(HostError::Journal);
    }
    if existing.released {
        return Ok(ReservationResult::Released {
            reservation_id: existing.id,
        });
    }
    if existing.state != "reserved"
        || !launch_is_pristine(
            connection,
            journal.connection_id.as_str(),
            existing.intent_id,
        )
        .await?
    {
        return Ok(ReservationResult::ReconcileRequired {
            reservation_id: existing.id,
        });
    }
    let Some(storage) = attempt.storage.as_ref() else {
        return Ok(ReservationResult::ReconcileRequired {
            reservation_id: existing.id,
        });
    };
    if !storage_budget::admission_matches_existing(connection, existing.id, storage).await? {
        return Ok(ReservationResult::ReconcileRequired {
            reservation_id: existing.id,
        });
    }
    let retained_memory = storage.plan().retained_memory_bytes()?;
    if !reservation_storage::guest_capacity_fits(
        connection,
        attempt.permit.as_ref().ok_or(HostError::Journal)?,
        request.memory_bytes,
        retained_memory,
        Some(existing.id),
    )
    .await?
    {
        return Ok(ReservationResult::AtCapacity);
    }
    if !admission_fits(connection, storage, request, Some(existing.id)).await? {
        return Ok(ReservationResult::AtCapacity);
    }
    validate_permit(request, attempt.permit.as_ref().ok_or(HostError::Journal)?)?;
    ensure_sample_is_new(connection, request.sample_generation_id).await?;
    consume_permit(&mut attempt.permit, request)?;
    record_sample_use(
        connection,
        existing.id,
        request.sample_generation_id,
        "resume",
    )
    .await?;
    let profile = storage_budget::current_profile(connection)
        .await?
        .ok_or(HostError::Journal)?;
    storage_budget::append_storage_sample(connection, existing.id, &profile, storage.sample())
        .await?;
    Ok(ReservationResult::Resumed {
        reservation_id: existing.id,
    })
}

async fn create_reservation(
    connection: &turso::Connection,
    journal: &ConnectionJournal,
    attempt: &mut ReservationAttempt<'_>,
) -> Result<ReservationResult, HostError> {
    let Some(storage) = attempt.storage.as_ref() else {
        return Ok(ReservationResult::AtCapacity);
    };
    let Some(profile) = storage_budget::current_profile(connection).await? else {
        return Ok(ReservationResult::AtCapacity);
    };
    let retained_memory = storage.plan().retained_memory_bytes()?;
    if !admission_fits(connection, storage, attempt.request, None).await?
        || !capacity_available(connection, attempt.request, retained_memory).await?
        || !reservation_storage::guest_capacity_fits(
            connection,
            attempt.permit.as_ref().ok_or(HostError::Journal)?,
            attempt.request.memory_bytes,
            retained_memory,
            None,
        )
        .await?
    {
        return Ok(ReservationResult::AtCapacity);
    }
    validate_permit(
        attempt.request,
        attempt.permit.as_ref().ok_or(HostError::Journal)?,
    )?;
    let (request_id, runner_name) = runner_assignment_fields(attempt.request.assignment.key());
    consume_permit(&mut attempt.permit, attempt.request)?;
    let intent_id =
        insert_launch_intent(connection, journal, attempt, request_id, runner_name).await?;
    let reservation_id =
        insert_capacity_reservation(connection, journal, attempt, intent_id).await?;
    record_sample_use(
        connection,
        reservation_id,
        attempt.request.sample_generation_id,
        "initial",
    )
    .await?;
    storage_budget::insert_reservation_storage(connection, reservation_id, &profile, storage)
        .await?;
    Ok(ReservationResult::Reserved { reservation_id })
}

async fn admission_fits(
    connection: &turso::Connection,
    storage: &StorageAdmission,
    request: ReservationRequest<'_>,
    existing_id: Option<i64>,
) -> Result<bool, HostError> {
    storage_budget::admission_fits(
        connection,
        storage,
        request.sample_generation_id,
        request.engine_id,
        request.docker_root_digest,
        request.memory_bytes,
        existing_id,
    )
    .await
}

fn consume_permit(
    permit: &mut Option<StartPermit>,
    request: ReservationRequest<'_>,
) -> Result<(), HostError> {
    let permit = permit.take().ok_or(HostError::Journal)?;
    if permit.consume_for_admission(request.engine_id, request.docker_root_digest) {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
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
