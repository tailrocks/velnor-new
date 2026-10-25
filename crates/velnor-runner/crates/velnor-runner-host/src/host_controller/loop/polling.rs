//! Positive polling and acquired-work retention for one connection.

use std::{path::Path, sync::Arc};

use bollard::Docker;

use crate::{
    config::{HostConfig, RepositoryConnection},
    error::HostError,
    journal::Journal,
    launch::{
        PositivePollContext as PositiveTurnContext, ReleaseReadSecret, SelectedEngineContext,
        VerifiedWorkerImages, ZeroHeaderMode,
    },
};

use super::state::PreparedConnection;
use acquired::start_acquired;
#[path = "polling/acquired.rs"]
mod acquired;
#[path = "polling/assigned_population.rs"]
mod assigned_population;
#[path = "polling/authorized_offers.rs"]
mod authorized_offers;
#[path = "polling/outcome.rs"]
mod outcome;
#[path = "polling/positive_offer.rs"]
mod positive_offer;
#[path = "polling/pre_jit.rs"]
mod pre_jit;
#[path = "polling/recovery.rs"]
mod recovery;
#[path = "polling/successor.rs"]
mod successor;

pub(crate) use outcome::{ControllerPollOutcome, PollKind};
use positive_offer::{PositiveOfferInputs, admit_positive_offer};
use recovery::{retain_authorization_pending, retain_bootstrap_pending, retain_ready_or_pending};

pub(super) struct PositivePollContext<'a> {
    pub(super) config: &'a HostConfig,
    pub(super) journal: &'a Journal,
    pub(super) docker: &'a Docker,
    pub(super) selected_engine: &'a SelectedEngineContext,
    pub(super) worker_images: &'a VerifiedWorkerImages,
    pub(super) state: &'a Path,
    pub(super) release_read_secret: &'a Arc<ReleaseReadSecret>,
    pub(super) admission_ready: bool,
    #[cfg(test)]
    pub(super) start_permit_provider: Option<&'a crate::launch::IntakeCompositionProvider>,
}

pub(super) async fn poll_zero_header_connection(
    prepared: &mut PreparedConnection,
    allocation: &crate::journal::HeaderAllocation,
    context: &PositivePollContext<'_>,
    cursor: i64,
    defer_available: bool,
    retired_binding: Option<&RepositoryConnection>,
) -> Result<crate::launch::ScopedPollOutcome, HostError> {
    let Some(credentials) = prepared.credentials.as_mut() else {
        return Err(HostError::Keychain);
    };
    let authorization = if defer_available {
        Some((
            context
                .config
                .connection(&prepared.binding.connection_id)
                .or(retired_binding)
                .ok_or(HostError::Config)?,
            credentials.github_pat.expose(),
        ))
    } else {
        None
    };
    let scoped = &prepared.scope;
    let set_id = prepared.set_id;
    let mode = if defer_available {
        ZeroHeaderMode::DeferAuthorizedOffers
    } else {
        ZeroHeaderMode::LifecycleOnly
    };
    crate::launch::poll_scoped_connection_once_with_mode(
        crate::launch::ScopedPollContext {
            link: &mut credentials.link,
            set_id,
            session: &mut credentials.session,
            scoped,
            session_intent_id: prepared.session_intent,
            allocation,
            cursor,
        },
        mode,
        async |poll| {
            let Some((connection, pat)) = authorization else {
                return Err(crate::scale_set::EnsureError::AdmissionClosed);
            };
            crate::launch::authorize_deferred_offers(connection, scoped, pat, set_id, poll).await
        },
    )
    .await
    .map_err(|_| HostError::Journal)
}

pub(super) async fn poll_positive_connection(
    prepared: &mut PreparedConnection,
    allocation: &crate::journal::HeaderAllocation,
    context: &PositivePollContext<'_>,
    pressure_sample: Option<crate::launch::FreshPressureSample>,
) -> Result<ControllerPollOutcome, HostError> {
    authorized_offers::resume(prepared, allocation, context).await?;
    let cursor = prepared.cursor_for_poll(std::time::Instant::now());
    let poll = Box::pin(positive_poll_message(
        prepared,
        allocation,
        context,
        cursor,
        pressure_sample,
    ))
    .await?;
    prepared.record_cursor_disposition(
        poll.message_id,
        poll.cursor_disposition,
        std::time::Instant::now(),
    );
    let mut outcome = ControllerPollOutcome::from_positive(poll);
    if let Some(acquired) = outcome.acquired.take() {
        start_acquired(prepared, acquired, context).await?;
    }
    Ok(outcome)
}

/// Continue an acquired request only when the plan names its existing reservation.
pub(super) async fn reconcile_assigned_population(
    prepared: &mut PreparedConnection,
    context: &PositivePollContext<'_>,
) -> Result<(), HostError> {
    assigned_population::reconcile(prepared, context).await
}

async fn positive_poll_message(
    prepared: &mut PreparedConnection,
    allocation: &crate::journal::HeaderAllocation,
    context: &PositivePollContext<'_>,
    cursor: i64,
    pressure_sample: Option<crate::launch::FreshPressureSample>,
) -> Result<crate::launch::PositivePollOutcome, HostError> {
    let Some(credentials) = prepared.credentials.as_mut() else {
        return Err(HostError::Keychain);
    };
    let connection = context
        .config
        .connection(&prepared.binding.connection_id)
        .ok_or(HostError::Config)?;
    let policy = context
        .config
        .validated_job_authority_policy(&prepared.binding.connection_id)?
        .ok_or(HostError::Config)?;
    let budget = context.config.resource_budget()?;
    let set_id = prepared.set_id;
    let scoped = &prepared.scope;
    let pat = credentials.github_pat.expose();
    let inputs = PositiveOfferInputs {
        context,
        connection,
        policy,
        budget,
        set_id,
        scoped,
        pat,
        allocated_header: allocation.target(),
    };
    let session_intent = prepared.session_intent;
    Box::pin(crate::launch::positive_poll_once_with_pressure(
        PositiveTurnContext {
            link: &mut credentials.link,
            set_id,
            session: &mut credentials.session,
            scoped,
            session_intent_id: session_intent,
            allocation,
            cursor,
        },
        pressure_sample,
        async |link, polled, queue_origin, queue_path, queue_token, admin_token| {
            admit_positive_offer(
                &inputs,
                link,
                polled,
                queue_origin,
                queue_path,
                queue_token,
                admin_token,
            )
            .await
        },
    ))
    .await
    .map_err(|_| HostError::Journal)
}
