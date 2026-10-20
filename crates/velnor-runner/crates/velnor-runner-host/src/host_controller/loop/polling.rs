//! Positive polling and acquired-work retention for one connection.

use std::path::Path;

use bollard::Docker;

use crate::{
    config::{HostConfig, RepositoryConnection},
    error::HostError,
    journal::Journal,
    launch::{
        Drive, GuestAdmission, ReleaseReadSecret, Rest, SelectedEngineContext,
        VerifiedWorkerImages, ZeroHeaderMode, positive_poll_once,
    },
};

use super::state::PreparedConnection;
use acquired::start_acquired;
#[path = "polling/acquired.rs"]
mod acquired;
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

const ASSIGNED_POPULATION_SCAN_LIMIT: u32 = 64;

pub(super) struct PositivePollContext<'a> {
    pub(super) config: &'a HostConfig,
    pub(super) journal: &'a Journal,
    pub(super) docker: &'a Docker,
    pub(super) selected_engine: &'a SelectedEngineContext,
    pub(super) worker_images: &'a VerifiedWorkerImages,
    pub(super) state: &'a Path,
    pub(super) release_read_secret: &'a ReleaseReadSecret,
    pub(super) admission_ready: bool,
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
        &mut credentials.link,
        set_id,
        &mut credentials.session,
        scoped,
        prepared.session_intent,
        allocation,
        cursor,
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
) -> Result<ControllerPollOutcome, HostError> {
    resume_authorized_offers(prepared, allocation, context).await?;
    let cursor = prepared.cursor_for_poll(std::time::Instant::now());
    let poll = positive_poll_message(prepared, allocation, context, cursor).await?;
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
    pre_jit::reconcile(prepared, context).await?;
    let Some(assigned_total) = prepared.assigned_total else {
        return Ok(());
    };
    let Some(connection) = context.config.connection(&prepared.binding.connection_id) else {
        return Ok(());
    };
    let Some(policy) = context
        .config
        .validated_job_authority_policy(&prepared.binding.connection_id)?
    else {
        return Ok(());
    };
    let reservations = prepared.scope.physical_runner_reservations().await?;
    let acquired = prepared
        .scope
        .acquired_authorized_requests(ASSIGNED_POPULATION_SCAN_LIMIT)
        .await?;
    let mut recovery_candidates = Vec::new();
    let mut successor_ready = Vec::new();
    for request in &acquired {
        if prepared
            .scope
            .acquired_successor_ready(request, policy.generation(), policy.policy_sha256())
            .await?
        {
            successor_ready.push(request.clone());
        }
    }
    for reservation in &reservations {
        let Some(request) = reservation.acquired_request.as_ref() else {
            continue;
        };
        if !acquired.contains(request)
            || reservation.reservation.state != crate::journal::ReservationState::Reserved
            || reservation.intent_state != Some(crate::journal::IntentState::Pending)
            || reservation.launch_phase != Some(crate::reconcile::LaunchPhase::ProvisioningHeld)
            || reservation.jit_state != crate::journal::PhysicalRunnerJitState::Absent
        {
            continue;
        }
        if let Some(candidate) = prepared
            .scope
            .acquired_recovery_candidate(
                request,
                reservation.reservation_id,
                policy.generation(),
                policy.policy_sha256(),
            )
            .await?
        {
            recovery_candidates.push(candidate);
        }
    }
    let plan = crate::host_controller::plan_assigned_population_with_successors(
        assigned_total,
        &prepared.binding.connection_id,
        prepared.set_id,
        &reservations,
        &acquired,
        &recovery_candidates,
        &successor_ready,
    );

    for action in plan.actions {
        let (request, reservation_id, candidate_index) = match action {
            crate::host_controller::AssignedPopulationAction::ContinueExisting {
                request,
                reservation_id,
                candidate_index,
            } => (request, reservation_id, candidate_index),
            crate::host_controller::AssignedPopulationAction::ReserveSuccessor { request } => {
                successor::reserve_and_start(prepared, context, &request, assigned_total).await?;
                continue;
            }
        };
        let candidate = recovery_candidates
            .get(candidate_index)
            .ok_or(HostError::Identity)?;
        if candidate.request() != &request || candidate.reservation_id() != reservation_id {
            return Err(HostError::Identity);
        }
        let Some(request_id) = request.request_id() else {
            return Err(HostError::Identity);
        };
        if prepared.pending_acquired_recovery.contains_key(&request_id) {
            continue;
        }
        let outcome = crate::launch::recover_acquired_scoped_offer(
            connection,
            policy,
            context.state,
            &prepared.scope,
            context.journal,
            &request,
            reservation_id,
            context.config.resource_budget()?,
            context.docker,
            context.release_read_secret,
            context.selected_engine,
        )
        .await;
        match outcome {
            Ok(crate::launch::steps::ScopedProvisioningOutcome::Recovered(recovery)) => {
                start_acquired(prepared, recovery.into_acquired(), context).await?;
            }
            Ok(crate::launch::steps::ScopedProvisioningOutcome::NoStart) => {}
            Ok(crate::launch::steps::ScopedProvisioningOutcome::KnownNoAcquire(_)) => {}
            Ok(crate::launch::steps::ScopedProvisioningOutcome::Acquired(_)) => {
                return Err(HostError::Ownership);
            }
            Err(error) => {
                eprintln!(
                    "The controller cannot continue recovery for this request. connection={} request={} reason={error}",
                    prepared.binding.connection_id, request_id
                );
            }
        }
    }
    Ok(())
}

async fn resume_authorized_offers(
    prepared: &mut PreparedConnection,
    allocation: &crate::journal::HeaderAllocation,
    context: &PositivePollContext<'_>,
) -> Result<(), HostError> {
    if !context.admission_ready || allocation.target() == 0 {
        return Ok(());
    }
    let offers = prepared.scope.authorized_offers(16).await?;
    let Some(offer) = offers.into_iter().next() else {
        return Ok(());
    };
    if offer.connection_id() != &prepared.binding.connection_id
        || offer.scale_set_id() != prepared.set_id
    {
        return Err(HostError::Identity);
    }
    let source_binding = prepared.scope.source_authorization_binding().await?;
    if offer.binding_digest() != source_binding.binding_digest()
        || source_binding.scale_set_id() != prepared.set_id
    {
        return Err(HostError::Identity);
    }
    let connection = context
        .config
        .connection(&prepared.binding.connection_id)
        .ok_or(HostError::Config)?;
    let policy = context
        .config
        .validated_job_authority_policy(&prepared.binding.connection_id)?
        .ok_or(HostError::Config)?;
    let Some(credentials) = prepared.credentials.as_mut() else {
        return Err(HostError::Keychain);
    };
    let Some(permit) = crate::launch::start_permit(
        context.docker,
        context.journal,
        context.config.resource_budget()?,
        context.release_read_secret,
    )
    .await
    else {
        return Ok(());
    };
    let (owner, repo) = connection
        .repository
        .split_once('/')
        .ok_or(HostError::Config)?;
    let mut drive = Drive::from_rest(
        prepared.set_id,
        credentials.session.message_queue_url.clone(),
        credentials.session.token().to_owned(),
        credentials.link.token().to_owned(),
        Rest {
            owner,
            repo,
            pat: credentials.github_pat.expose(),
            resource_budget: Some(context.config.resource_budget()?),
            static_capacity: true,
            guest_admission: GuestAdmission::Unavailable,
            connection: Some(connection),
            authority_policy: Some(policy),
            host_journal: Some(context.journal),
            release_read_secret: Some(context.release_read_secret),
            scoped_journal: Some(&prepared.scope),
            allocated_header: allocation.target(),
            source_authorized: true,
            admission_state: Some(context.state),
            worker_images: Some(context.worker_images),
            selected_engine: Some(context.selected_engine),
            guest_image: Some(&prepared.guest_image),
        },
    );
    drive.docker_engine_id = Some(context.selected_engine.engine_id().to_owned());
    let admin = credentials.link.base().to_owned();
    let mut lane = crate::launch::HostLane::new(&mut credentials.link, admin, None);
    let outcome = crate::launch::steps::acquire_authorized_request(
        &mut lane,
        &drive,
        offer.request_id(),
        permit,
        context.selected_engine,
    )
    .await
    .map_err(|_| HostError::Journal)?;
    if let crate::launch::steps::ScopedProvisioningOutcome::Acquired(acquired) = outcome {
        start_acquired(prepared, acquired, context).await?;
    }
    Ok(())
}

async fn positive_poll_message(
    prepared: &mut PreparedConnection,
    allocation: &crate::journal::HeaderAllocation,
    context: &PositivePollContext<'_>,
    cursor: i64,
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
    let guest_image = &prepared.guest_image;
    let pat = credentials.github_pat.expose();
    let inputs = PositiveOfferInputs {
        context,
        connection,
        policy,
        budget,
        set_id,
        scoped,
        guest_image,
        pat,
        allocated_header: allocation.target(),
    };
    let session_intent = prepared.session_intent;
    positive_poll_once(
        &mut credentials.link,
        set_id,
        &mut credentials.session,
        scoped,
        session_intent,
        allocation,
        cursor,
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
    )
    .await
    .map_err(|_| HostError::Journal)
}
