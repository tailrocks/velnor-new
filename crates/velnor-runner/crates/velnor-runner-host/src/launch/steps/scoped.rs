//! One source-approved request and one charged physical runner.

use sha2::{Digest, Sha256};
use velnor_runner_github::{Poll, Transport};

use crate::journal::{AssignmentKey, ConnectionJournal};
use crate::launch::{Drive, Lane};
use crate::scale_set::EnsureError;

use super::{assignment, source_blocked};

mod acquire;
mod runner;
#[cfg(test)]
pub(crate) use runner::run_native_quarantine_composition;
pub(crate) use runner::{AcquiredProvisioning, CheckedAcquiredRecovery};

#[cfg(test)]
pub(crate) use runner::{RunnerCreateFailure, runner_create_409};

pub(crate) enum ScopedProvisioningOutcome {
    NoStart,
    KnownNoAcquire(KnownNoAcquire),
    Acquired(runner::AcquiredProvisioning),
    Recovered(runner::CheckedAcquiredRecovery),
}

pub(crate) struct KnownNoAcquire {
    request: crate::journal::AssignmentIdentity,
    state: crate::journal::RequestAcquireState,
    reservation_released: bool,
}

impl KnownNoAcquire {
    pub(crate) fn new(
        request: crate::journal::AssignmentIdentity,
        state: crate::journal::RequestAcquireState,
        reservation_released: bool,
    ) -> Self {
        Self {
            request,
            state,
            reservation_released,
        }
    }

    #[cfg(test)]
    pub(crate) fn test_fixture(
        request: crate::journal::AssignmentIdentity,
        state: crate::journal::RequestAcquireState,
        reservation_released: bool,
    ) -> Self {
        Self::new(request, state, reservation_released)
    }

    pub(crate) fn request(&self) -> &crate::journal::AssignmentIdentity {
        &self.request
    }

    pub(crate) const fn state(&self) -> crate::journal::RequestAcquireState {
        self.state
    }

    pub(crate) const fn reservation_released(&self) -> bool {
        self.reservation_released
    }

    pub(crate) fn matches_poll(
        &self,
        polled: &Poll,
        scoped: &ConnectionJournal,
        set_id: i64,
    ) -> bool {
        let Ok(Some((_, request_id))) = assignment(polled) else {
            return false;
        };
        self.request.connection_id() == scoped.connection_id()
            && self.request.scale_set_id() == set_id
            && self.request.request_id() == Some(request_id)
    }
}

pub(crate) async fn acquire_scoped<T>(
    lane: &mut T,
    ctx: &Drive,
    polled: &Poll,
    permit: crate::launch::StartPermit,
    expected_engine: &crate::launch::SelectedEngineContext,
) -> Result<ScopedProvisioningOutcome, EnsureError>
where
    T: Transport + Lane,
{
    if !ctx.source_authorized {
        return Err(source_blocked(
            super::source::SourceBlock::ClosedWorldUnproven,
        ));
    }
    let Some((_batch, request_id)) = assignment(polled)? else {
        return Ok(ScopedProvisioningOutcome::NoStart);
    };
    acquire_authorized_request(lane, ctx, request_id, permit, expected_engine).await
}

/// Acquire one exact source-authorized request without a queue `Poll` object.
pub(crate) async fn acquire_authorized_request<T>(
    lane: &mut T,
    ctx: &Drive,
    request_id: i64,
    permit: crate::launch::StartPermit,
    expected_engine: &crate::launch::SelectedEngineContext,
) -> Result<ScopedProvisioningOutcome, EnsureError>
where
    T: Transport + Lane,
{
    let (connection, scoped, state) = scoped_context(ctx)?;
    let authority_policy = ctx
        .authority_policy
        .as_ref()
        .ok_or(EnsureError::Uncertain)?;
    if !ctx.source_authorized || request_id <= 0 {
        return Err(source_blocked(
            super::source::SourceBlock::ClosedWorldUnproven,
        ));
    }
    validate_scope_binding(connection, scoped, ctx.set_id).await?;
    let _admission = crate::launch::lock_connection(state, connection)?;
    let request = scoped
        .assignment_identity(AssignmentKey::AssignedRequest { request_id })
        .await
        .map_err(super::map_journal)?;
    if let Some((state, reservation_released)) = scoped
        .known_no_acquire_disposition(&request)
        .await
        .map_err(super::map_journal)?
    {
        return Ok(ScopedProvisioningOutcome::KnownNoAcquire(
            KnownNoAcquire::new(request, state, reservation_released),
        ));
    }
    acquire::require_authorized(scoped, &request).await?;
    let Some(profile) = scoped
        .current_storage_profile()
        .await
        .map_err(super::map_journal)?
    else {
        return Ok(ScopedProvisioningOutcome::NoStart);
    };
    let Some(storage) = permit.storage_provisioning_admission(
        &profile,
        expected_engine.engine_id(),
        expected_engine.docker_root_digest(),
    ) else {
        return Ok(ScopedProvisioningOutcome::NoStart);
    };
    lane.on_admin()?;

    let outcome =
        runner::reserve_and_claim(scoped, permit, storage, &request, authority_policy).await?;
    let runner::ReservationOutcome::AcquireClaimed(physical, claim) = outcome else {
        return Ok(ScopedProvisioningOutcome::NoStart);
    };
    let disposition = acquire::run_claimed(lane, &ctx.queue_token, scoped, claim).await?;
    map_acquire_disposition(disposition, physical, request)
}

fn map_acquire_disposition(
    disposition: acquire::AcquireDisposition,
    physical: runner::PhysicalIntent,
    request: crate::journal::AssignmentIdentity,
) -> Result<ScopedProvisioningOutcome, EnsureError> {
    match disposition {
        acquire::AcquireDisposition::Acquired => Ok(ScopedProvisioningOutcome::Acquired(
            runner::AcquiredProvisioning::after_recorded_acquire(physical, request),
        )),
        acquire::AcquireDisposition::NoEffect => Err(EnsureError::Unexpected {
            status: 0,
            step: "acquire",
        }),
        acquire::AcquireDisposition::NoEffectSettled => {
            Ok(ScopedProvisioningOutcome::KnownNoAcquire(
                KnownNoAcquire::new(request, crate::journal::RequestAcquireState::NoEffect, true),
            ))
        }
        acquire::AcquireDisposition::NoEffectHeld => Ok(ScopedProvisioningOutcome::KnownNoAcquire(
            KnownNoAcquire::new(
                request,
                crate::journal::RequestAcquireState::NoEffect,
                false,
            ),
        )),
        acquire::AcquireDisposition::Rejected => Err(EnsureError::Rejected),
        acquire::AcquireDisposition::RejectedSettled => {
            Ok(ScopedProvisioningOutcome::KnownNoAcquire(
                KnownNoAcquire::new(request, crate::journal::RequestAcquireState::Rejected, true),
            ))
        }
        acquire::AcquireDisposition::RejectedHeld => Ok(ScopedProvisioningOutcome::KnownNoAcquire(
            KnownNoAcquire::new(
                request,
                crate::journal::RequestAcquireState::Rejected,
                false,
            ),
        )),
        acquire::AcquireDisposition::Uncertain => Err(EnsureError::Uncertain),
    }
}

fn scoped_context(
    ctx: &Drive,
) -> Result<
    (
        &crate::config::RepositoryConnection,
        &ConnectionJournal,
        &std::path::Path,
    ),
    EnsureError,
> {
    match (
        ctx.connection.as_ref(),
        ctx.scoped_journal.as_ref(),
        ctx.admission_state.as_deref(),
    ) {
        (Some(connection), Some(scoped), Some(state)) => Ok((connection, scoped, state)),
        _ => Err(source_blocked(
            super::source::SourceBlock::ClosedWorldUnproven,
        )),
    }
}

async fn validate_scope_binding(
    connection: &crate::config::RepositoryConnection,
    scoped: &ConnectionJournal,
    set_id: i64,
) -> Result<(), EnsureError> {
    let binding = scoped
        .source_authorization_binding()
        .await
        .map_err(super::map_journal)?;
    let digest = canonical_binding_digest(connection)?;
    if binding.connection_id() == &connection.connection_id
        && binding.connection_id() == scoped.connection_id()
        && binding.scale_set_id() == set_id
        && binding.binding_digest() == digest
    {
        Ok(())
    } else {
        Err(EnsureError::Uncertain)
    }
}

fn canonical_binding_digest(
    connection: &crate::config::RepositoryConnection,
) -> Result<String, EnsureError> {
    let identity = connection
        .canonical_identity_bytes()
        .map_err(|_| EnsureError::Uncertain)?;
    let digest = Sha256::digest(identity);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").map_err(|_| EnsureError::Uncertain)?;
    }
    Ok(output)
}
