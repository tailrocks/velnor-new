//! Durable reservation and one-shot creation of a verified queue session.

use std::time::Instant;

use tokio::sync::watch;
use velnor_runner_github::VerifiedPoolSessionAdmin;
use velnor_runner_github::policy::PoolRegistrationScope;
use velnor_runner_host::BoundedDiscoveryTransport;
use velnor_runner_journal::journal::{
    Journal, ReplayRoute, ScaleSetSessionClaim, ScaleSetSessionIdentity,
};

use crate::linux::{LinuxAdmissionState, LinuxLaunchContext};

use super::{
    ActiveSession, DeadlineBoundTransport, Protocol, ShutdownGate, cutoff, observe_cutoff,
};
use cutoff::DispatchFence;

pub(in crate::linux) async fn create_session_if_verified(
    context: &LinuxLaunchContext,
    journal: &Journal,
    admin: Option<VerifiedPoolSessionAdmin>,
    shutdown: &mut watch::Receiver<Option<Instant>>,
    cutoff: &mut Option<Instant>,
) -> (Option<ActiveSession>, Option<LinuxAdmissionState>) {
    let Some(admin) = admin else {
        return (None, None);
    };
    match create_verified_session(context, journal, admin, shutdown, cutoff).await {
        Ok(active) => (
            Some(active),
            Some(LinuxAdmissionState::VerifiedSessionReady),
        ),
        Err(state) => (None, Some(state)),
    }
}

async fn create_verified_session(
    context: &LinuxLaunchContext,
    journal: &Journal,
    admin: VerifiedPoolSessionAdmin,
    shutdown: &mut watch::Receiver<Option<Instant>>,
    cutoff: &mut Option<Instant>,
) -> Result<ActiveSession, LinuxAdmissionState> {
    let binding = admin.binding().clone();
    let (owner, repository) = session_target(context, &binding)?;
    let mut gate = ShutdownGate {
        receiver: shutdown,
        cutoff,
    };
    observe_cutoff(context, journal, gate.receiver, gate.cutoff).await;
    if gate.cutoff.is_some() {
        return Err(LinuxAdmissionState::ShutdownBeforePreflight);
    }
    let (identity, intent_id) =
        reserve_session_intent(context, journal, &binding, &owner, &repository, &mut gate).await?;
    let (admin, session) = dispatch_create(context, journal, admin, &owner, &mut gate).await?;
    let session = session.map_err(|_| LinuxAdmissionState::SessionEffectUncertain)?;
    let recorded = cutoff::bounded_persisting(
        journal,
        &mut gate,
        context.drain_timeout(),
        None,
        journal.record_scale_set_session_created(intent_id, session.session_id()),
    )
    .await;
    if !matches!(recorded, Some(Ok(()))) {
        return Err(LinuxAdmissionState::SessionEffectUncertain);
    }
    Ok(ActiveSession {
        binding,
        identity,
        intent_id,
        protocol: std::sync::Arc::new(std::sync::Mutex::new(Protocol {
            admin,
            session,
            assigned_demand: None,
        })),
    })
}

fn session_target(
    context: &LinuxLaunchContext,
    binding: &velnor_runner_github::policy::PoolBinding,
) -> Result<(String, String), LinuxAdmissionState> {
    let (owner, repository) = match &binding.registration_scope {
        PoolRegistrationScope::Repository { owner, repository } => {
            (owner.clone(), repository.clone())
        }
        PoolRegistrationScope::Organization { .. } => {
            return Err(LinuxAdmissionState::SessionCloseUnsupportedScope);
        }
    };
    if !context
        .snapshot
        .config()
        .github
        .repository
        .eq_ignore_ascii_case(&binding.repository_full_name)
        || format!("{owner}/{repository}") != binding.repository_full_name
        || binding.scale_set_name != context.snapshot.scale_set_binding().scale_set_name
        || binding.actions_runner_group_id != context.snapshot.scale_set_binding().runner_group_id
    {
        return Err(LinuxAdmissionState::PoolPreflightUnavailable);
    }
    Ok((owner, repository))
}

async fn reserve_session_intent(
    context: &LinuxLaunchContext,
    journal: &Journal,
    binding: &velnor_runner_github::policy::PoolBinding,
    owner: &str,
    repository: &str,
    gate: &mut ShutdownGate<'_>,
) -> Result<(ScaleSetSessionIdentity, i64), LinuxAdmissionState> {
    let route = replay_route(binding, owner, repository);
    let identity =
        ScaleSetSessionIdentity::new(route, binding.repository_id, &binding.repository_full_name)
            .map_err(|_| LinuxAdmissionState::PoolPreflightUnavailable)?;
    let claim = cutoff::bounded_persisting(
        journal,
        gate,
        context.drain_timeout(),
        None,
        journal.reserve_scale_set_session_if_accepting(&identity),
    )
    .await
    .ok_or(LinuxAdmissionState::SessionEffectUncertain)?
    .map_err(|_| LinuxAdmissionState::PoolPreflightUnavailable)?;
    let intent_id = match claim {
        ScaleSetSessionClaim::Reserved(id) => id,
        ScaleSetSessionClaim::Existing(_) => return Err(LinuxAdmissionState::ExistingSessionHeld),
        ScaleSetSessionClaim::Draining => return Err(LinuxAdmissionState::ShutdownBeforePreflight),
    };
    observe_cutoff(context, journal, gate.receiver, gate.cutoff).await;
    if gate.cutoff.is_some() {
        let rejected = cutoff::bounded_persisting(
            journal,
            gate,
            context.drain_timeout(),
            None,
            journal.record_scale_set_session_rejected(intent_id),
        )
        .await;
        if !matches!(rejected, Some(Ok(()))) {
            return Err(LinuxAdmissionState::SessionEffectUncertain);
        }
        return Err(LinuxAdmissionState::ShutdownBeforePreflight);
    }
    Ok((identity, intent_id))
}

async fn dispatch_create(
    context: &LinuxLaunchContext,
    journal: &Journal,
    mut admin: VerifiedPoolSessionAdmin,
    owner: &str,
    gate: &mut ShutdownGate<'_>,
) -> Result<
    (
        VerifiedPoolSessionAdmin,
        Result<velnor_runner_github::VerifiedQueueSession, velnor_runner_github::SessionError>,
    ),
    LinuxAdmissionState,
> {
    let dispatch = DispatchFence::new();
    let worker_dispatch = dispatch.clone();
    let worker_shutdown = gate.receiver.clone();
    let worker_cutoff = *gate.cutoff;
    let owner = owner.to_owned();
    let create = tokio::task::spawn_blocking(move || {
        if !worker_dispatch.allowed(&worker_shutdown, worker_cutoff) {
            return (admin, Err(velnor_runner_github::SessionError::Uncertain));
        }
        if !worker_dispatch.begin(&worker_shutdown, worker_cutoff) {
            return (admin, Err(velnor_runner_github::SessionError::Uncertain));
        }
        let mut transport = DeadlineBoundTransport::new(
            BoundedDiscoveryTransport::new(),
            worker_dispatch.clone(),
            worker_shutdown.clone(),
            worker_cutoff,
        );
        let session = admin.create_session(&mut transport, &owner);
        (admin, session)
    });
    let create = cutoff::bounded_protocol(
        journal,
        gate,
        context.drain_timeout(),
        None,
        dispatch,
        create,
    )
    .await;
    let Some(create) = create else {
        return Err(LinuxAdmissionState::SessionEffectUncertain);
    };
    let create = create.map_err(|_| LinuxAdmissionState::SessionEffectUncertain)?;
    observe_cutoff(context, journal, gate.receiver, gate.cutoff).await;
    Ok(create)
}

fn replay_route<'a>(
    binding: &'a velnor_runner_github::policy::PoolBinding,
    owner: &'a str,
    repository: &'a str,
) -> ReplayRoute<'a> {
    ReplayRoute {
        destination: "https://api.github.com",
        registration_scope: "repository",
        owner,
        repository,
        runner_group_id: binding.actions_runner_group_id,
        runner_group_name: &binding.actions_runner_group_name,
        scale_set_id: binding.scale_set_id,
        scale_set_name: &binding.scale_set_name,
    }
}
