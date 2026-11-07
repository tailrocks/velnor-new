// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::Cell;

use velnor_runner_github::{
    AcquireOutcome, RefreshGate, SessionError, Transport, WireError, acquire,
};

use crate::journal::{
    AssignedAcquireResult, AssignmentIdentity, AssignmentKey, ConnectionJournal,
    CreatedScaleSetResponse, Journal, RequestAcquireState, ScaleSetCreateRequest,
};
use crate::scale_set::EnsureError;

pub async fn resume_deferred_offer_acquire_with_transport<T>(
    journal: &Journal,
    connection: &crate::config::RepositoryConnection,
    scale_set_id: i64,
    token: &str,
    batch: &velnor_runner_github::ParsedBatch,
    request_id: i64,
    transport: &mut T,
) -> Result<bool, EnsureError>
where
    T: Transport + ?Sized,
{
    let scoped = journal
        .connection_scope(connection)
        .await
        .map_err(super::super::map_journal)?;
    if scoped
        .scale_set()
        .await
        .map_err(super::super::map_journal)?
        .is_none()
    {
        let create =
            ScaleSetCreateRequest::new(connection, 1).map_err(super::super::map_journal)?;
        let record = scoped
            .prepare_scale_set_create(&create)
            .await
            .map_err(super::super::map_journal)?;
        let claim = scoped
            .claim_scale_set_create(&record)
            .await
            .map_err(super::super::map_journal)?
            .ok_or(EnsureError::Uncertain)?;
        let created = CreatedScaleSetResponse::verified(
            &create,
            scale_set_id,
            connection.scale_set.name(),
            connection.scale_set.labels(),
            true,
        )
        .map_err(super::super::map_journal)?;
        scoped
            .record_scale_set_created(claim, created)
            .await
            .map_err(super::super::map_journal)?;
    }
    super::super::offer_authorization::authorize_offer_with_transport(
        Some(connection),
        Some(&scoped),
        token,
        batch,
        request_id,
        transport,
    )
    .await?;
    let offer = scoped
        .authorized_offers(16)
        .await
        .map_err(super::super::map_journal)?
        .into_iter()
        .find(|item| item.request_id() == request_id)
        .ok_or(EnsureError::Uncertain)?;
    let identity = scoped
        .assignment_identity(AssignmentKey::AssignedRequest { request_id })
        .await
        .map_err(super::super::map_journal)?;
    let stored_already_assigned = require_authorized(&scoped, &identity).await?;
    if !scoped
        .claim_authorized_request_acquire(&identity)
        .await
        .map_err(super::super::map_journal)?
    {
        return Err(EnsureError::Uncertain);
    }
    let disposition = run_claimed(
        transport,
        token,
        &scoped,
        ClaimedAcquire::new(identity),
        offer.already_assigned() || stored_already_assigned,
    )
    .await?;
    if disposition != AcquireDisposition::Acquired {
        return Err(EnsureError::Uncertain);
    }
    Ok(offer.already_assigned() && stored_already_assigned)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AcquireDisposition {
    Acquired,
    NoEffect,
    NoEffectSettled,
    NoEffectHeld,
    Rejected,
    RejectedSettled,
    RejectedHeld,
    Uncertain,
}

pub(super) struct ClaimedAcquire {
    identity: AssignmentIdentity,
}

impl ClaimedAcquire {
    pub(super) fn new(identity: AssignmentIdentity) -> Self {
        Self { identity }
    }
}

pub(super) async fn require_authorized(
    scoped: &ConnectionJournal,
    identity: &AssignmentIdentity,
) -> Result<bool, EnsureError> {
    let state = scoped
        .request_acquire_state(identity)
        .await
        .map_err(super::super::map_journal)?;
    if state == Some(RequestAcquireState::Authorized) {
        scoped
            .request_already_assigned(identity)
            .await
            .map_err(super::super::map_journal)
    } else {
        Err(EnsureError::Uncertain)
    }
}

pub(super) async fn run_claimed<T>(
    lane: &mut T,
    queue_token: &str,
    scoped: &ConnectionJournal,
    claim: ClaimedAcquire,
    already_assigned: bool,
) -> Result<AcquireDisposition, EnsureError>
where
    T: Transport + ?Sized,
{
    let identity = claim.identity;
    let request_id = identity.request_id().ok_or(EnsureError::Uncertain)?;
    let binding = scoped
        .source_authorization_binding()
        .await
        .map_err(super::super::map_journal)?;
    if identity.connection_id() != scoped.connection_id()
        || identity.connection_id() != binding.connection_id()
        || identity.scale_set_id() != binding.scale_set_id()
        || scoped
            .request_acquire_state(&identity)
            .await
            .map_err(super::super::map_journal)?
            != Some(RequestAcquireState::Requested)
    {
        return Err(EnsureError::Uncertain);
    }
    if already_assigned {
        scoped
            .record_request_acquire(&identity, AssignedAcquireResult::Returned(vec![request_id]))
            .await
            .map_err(super::super::map_journal)?;
        return Ok(AcquireDisposition::Acquired);
    }
    let gate = RefreshGate::new();
    let refresh_requested = Cell::new(false);
    let result = acquire(
        lane,
        identity.scale_set_id(),
        &[request_id],
        &[],
        queue_token,
        &gate,
        || {
            refresh_requested.set(true);
            Err::<(), WireError>(WireError::RefreshExhausted)
        },
    );
    let (durable, disposition) = if refresh_requested.get() {
        (
            AssignedAcquireResult::Uncertain,
            AcquireDisposition::Uncertain,
        )
    } else {
        classify(&result, request_id)
    };
    let disposition = record_acquire_disposition(scoped, &identity, durable, disposition).await?;
    match disposition {
        AcquireDisposition::Uncertain if refresh_requested.get() => Err(EnsureError::Uncertain),
        AcquireDisposition::Uncertain => match result {
            Err(error) => Err(crate::listen::map_listen(error)),
            Ok(_) => Err(EnsureError::Uncertain),
        },
        known => Ok(known),
    }
}

async fn record_acquire_disposition(
    scoped: &ConnectionJournal,
    identity: &AssignmentIdentity,
    durable: AssignedAcquireResult,
    disposition: AcquireDisposition,
) -> Result<AcquireDisposition, EnsureError> {
    Ok(match disposition {
        AcquireDisposition::NoEffect => {
            if scoped
                .record_request_acquire_and_settle_unused(identity, durable)
                .await
                .map_err(super::super::map_journal)?
            {
                AcquireDisposition::NoEffectSettled
            } else {
                AcquireDisposition::NoEffectHeld
            }
        }
        AcquireDisposition::Rejected => {
            if scoped
                .record_request_acquire_and_settle_unused(identity, durable)
                .await
                .map_err(super::super::map_journal)?
            {
                AcquireDisposition::RejectedSettled
            } else {
                AcquireDisposition::RejectedHeld
            }
        }
        _ => {
            scoped
                .record_request_acquire(identity, durable)
                .await
                .map_err(super::super::map_journal)?;
            disposition
        }
    })
}

pub(super) fn classify(
    result: &Result<AcquireOutcome, SessionError>,
    request_id: i64,
) -> (AssignedAcquireResult, AcquireDisposition) {
    match result {
        Ok(AcquireOutcome::Acquired(ids)) if ids.as_slice() == [request_id] => (
            AssignedAcquireResult::Returned(ids.clone()),
            AcquireDisposition::Acquired,
        ),
        Ok(AcquireOutcome::Acquired(ids)) if ids.is_empty() => (
            AssignedAcquireResult::NoEffect,
            AcquireDisposition::NoEffect,
        ),
        Ok(AcquireOutcome::Acquired(ids)) => (
            AssignedAcquireResult::Returned(ids.clone()),
            AcquireDisposition::Uncertain,
        ),
        Ok(AcquireOutcome::Noop) => (
            AssignedAcquireResult::AlreadyAcquired,
            AcquireDisposition::Uncertain,
        ),
        Err(SessionError::Wire(WireError::RefreshExhausted)) => (
            AssignedAcquireResult::Uncertain,
            AcquireDisposition::Uncertain,
        ),
        Err(error) if error.certainty() == velnor_runner_github::Certainty::Definite => (
            AssignedAcquireResult::Rejected,
            AcquireDisposition::Rejected,
        ),
        Err(_) => (
            AssignedAcquireResult::Uncertain,
            AcquireDisposition::Uncertain,
        ),
    }
}
