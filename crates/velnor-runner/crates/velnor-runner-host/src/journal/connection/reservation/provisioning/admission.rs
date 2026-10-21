use std::time::Instant;

use crate::error::HostError;
use crate::journal::connection::AssignmentKey;
use crate::journal::connection::storage::has_ambiguous_legacy_work;
#[cfg(test)]
use crate::journal::connection::storage::in_transaction;
use crate::worker::{WorkerContainerRole, WorkerIdentityNames, WorkerVolumeRole};

use super::super::admission::{
    ReservationRequest, ReservationResult, ensure_sample_is_new, record_sample_use, validate_permit,
};
use super::super::storage::ExistingReservation;
use super::super::storage::{capacity_limits, capacity_usage, has_unreserved_scoped_launch};
use super::super::storage_budget::{
    StorageInodePolicy, StorageMedium, StorageProfile, StorageProvisioningAdmission,
};
use crate::journal::connection::ConnectionJournal;
use crate::launch::StartPermit;

mod capacity;
mod record;
use capacity::{
    capacity_fits, disk_pool_fits, has_legacy_active_reservation, profile_retained_memory,
};
#[cfg(test)]
pub(super) use capacity::{charge_fits_pool, checked_required_charge};
pub(super) use capacity::{metadata_capacity_fits, sql_i64};
pub(super) use record::container_role;
use record::{insert_capacity_reservation, insert_launch_intent, insert_provisioning_rows};

#[cfg(test)]
pub(super) async fn reserve(
    journal: &ConnectionJournal,
    request: ReservationRequest<'_>,
    permit: StartPermit,
    admission: StorageProvisioningAdmission,
) -> Result<ReservationResult, HostError> {
    in_transaction(journal.journal_ref(), async |connection| {
        let mut permit = Some(permit);
        reserve_in_transaction(connection, journal, request, &mut permit, &admission).await
    })
    .await
}

pub(super) async fn reserve_in_transaction(
    connection: &turso::Connection,
    journal: &ConnectionJournal,
    request: ReservationRequest<'_>,
    permit: &mut Option<StartPermit>,
    admission: &StorageProvisioningAdmission,
) -> Result<ReservationResult, HostError> {
    super::super::storage::validate_request(request)?;
    let nonce = reservation_nonce(request.assignment.key())?;
    validate_permit(request, permit.as_ref().ok_or(HostError::Journal)?)?;
    journal.validate_scope(connection).await?;
    validate_scale_set(connection, journal, request).await?;
    if has_ambiguous_legacy_work(connection).await?
        || has_unreserved_scoped_launch(connection).await?
    {
        return Err(HostError::Journal);
    }
    if let Some(existing) = existing_reservation(connection, journal, request).await? {
        validate_existing(request, journal, &existing)?;
        return Ok(if existing.released {
            ReservationResult::Released {
                reservation_id: existing.id,
            }
        } else {
            ReservationResult::ReconcileRequired {
                reservation_id: existing.id,
            }
        });
    }
    if has_legacy_active_reservation(connection).await? {
        return Ok(ReservationResult::AtCapacity);
    }

    let Some(profile) =
        validated_profile_for_admission(connection, request, permit, admission).await?
    else {
        return Ok(ReservationResult::AtCapacity);
    };
    ensure_sample_is_new(connection, request.sample_generation_id).await?;
    let permit_value = permit.take().ok_or(HostError::Journal)?;
    if !permit_value.consume_for_admission(request.engine_id, request.docker_root_digest) {
        return Err(HostError::Journal);
    }
    let names = WorkerIdentityNames::from_nonce(nonce)?;
    let runner_name = names.remote_runner_name()?;
    let intent_id = insert_launch_intent(connection, journal, request, &runner_name).await?;
    let reservation_id =
        insert_capacity_reservation(connection, journal, request, intent_id).await?;
    record_sample_use(
        connection,
        reservation_id,
        request.sample_generation_id,
        "initial",
    )
    .await?;
    insert_provisioning_rows(
        connection,
        reservation_id,
        intent_id,
        request,
        &profile,
        admission,
        &names,
    )
    .await?;
    Ok(ReservationResult::Reserved { reservation_id })
}

async fn validated_profile_for_admission(
    connection: &turso::Connection,
    request: ReservationRequest<'_>,
    permit: &Option<StartPermit>,
    admission: &StorageProvisioningAdmission,
) -> Result<Option<super::super::storage_budget::StorageProfile>, HostError> {
    let Some(profile) = super::super::storage_budget::current_profile(connection).await? else {
        return Err(HostError::Journal);
    };
    profile.validate_provisioning_contract()?;
    let retained_memory = profile_retained_memory(&profile)?;
    admission.validate_for(
        &profile,
        request.sample_generation_id,
        request.engine_id,
        request.docker_root_digest,
        request.memory_bytes,
        Instant::now(),
    )?;
    if !disk_pool_fits(connection, &profile, admission.sample()).await?
        || !capacity_fits(connection, request, retained_memory).await?
        || !super::super::storage::guest_capacity_fits(
            connection,
            permit.as_ref().ok_or(HostError::Journal)?,
            request.memory_bytes,
            retained_memory,
            None,
        )
        .await?
    {
        return Ok(None);
    }
    Ok(Some(profile))
}

fn reservation_nonce(key: &AssignmentKey) -> Result<&str, HostError> {
    let AssignmentKey::Remote { kind, subject } = key else {
        return Err(HostError::Identity);
    };
    if kind != "runner"
        || subject.len() != 32
        || !subject
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(HostError::Identity);
    }
    Ok(subject)
}

async fn validate_scale_set(
    connection: &turso::Connection,
    journal: &ConnectionJournal,
    request: ReservationRequest<'_>,
) -> Result<(), HostError> {
    if request.assignment.connection_id() != journal.connection_id() {
        return Err(HostError::Journal);
    }
    let mut rows = connection
        .query(
            "SELECT scale_set_id FROM connection_runtime_bindings WHERE connection_id = ?1",
            [journal.connection_id().as_str()],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let id: i64 = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    if id == request.assignment.scale_set_id() {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

async fn existing_reservation(
    connection: &turso::Connection,
    journal: &ConnectionJournal,
    request: ReservationRequest<'_>,
) -> Result<Option<ExistingReservation>, HostError> {
    super::super::storage::reservation_for_assignment(
        connection,
        journal.connection_id().as_str(),
        request.assignment,
    )
    .await
}

fn validate_existing(
    request: ReservationRequest<'_>,
    journal: &ConnectionJournal,
    existing: &ExistingReservation,
) -> Result<(), HostError> {
    if existing.engine_id != request.engine_id
        || existing.docker_root_digest != request.docker_root_digest
        || existing.binding_digest != journal.binding_digest()
        || existing.cpu_millicores
            != i64::try_from(request.cpu_millicores).map_err(|_| HostError::Journal)?
        || existing.memory_bytes
            != i64::try_from(request.memory_bytes).map_err(|_| HostError::Journal)?
    {
        Err(HostError::Journal)
    } else {
        Ok(())
    }
}

#[cfg(test)]
#[path = "admission_tests.rs"]
mod tests;
