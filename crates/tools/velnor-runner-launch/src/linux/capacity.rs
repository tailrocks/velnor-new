//! Durable global launch reservations for typed Linux session work.

use std::future::Future;
use std::num::NonZeroU32;

use velnor_runner_github::EncodedJit;
use velnor_runner_github::policy::VerifiedJobTrust;
use velnor_runner_host::{
    HostError, RunnerImageProfile, stage::PairEngine, worker::new_worker_volume,
};
use velnor_runner_journal::journal::{
    AssignedPopulationObservation, BoundCapacityClaim, Journal, JournalDockerDaemonBinding,
    ReplayRoute, ScopedAssignedLaunchIdentity, ScopedLaunchIdentity,
};

use super::worker::start_worker_pair;

/// One successfully reserved launch and its durable runner name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ReservedLaunch {
    pub(super) id: i64,
    pub(super) runner_name: String,
}

/// Whether capacity allowed the offer to cross the external-effect boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReserveOutcome {
    /// A new row was committed and its external effect may now be attempted.
    Reserved,
    /// This exact offer was already reserved; it does not authorize replay.
    Existing,
    /// A durable drain fence blocks a new reservation.
    Draining,
    /// Global occupancy is at the configured maximum.
    CapacityFull,
}

/// Durable result of the one-shot post-reservation effect sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LaunchEffectOutcome {
    /// Every requested effect and the terminal row update completed.
    Done,
    /// No external effect was authorized or dispatched.
    NotDispatched,
    /// An effect or its durable outcome may have occurred; capacity stays held.
    Uncertain,
}

/// Exact durable key and count snapshot for one generic assigned runner slot.
pub(super) struct AssignedSlotIdentity<'a> {
    pub(super) route: ReplayRoute<'a>,
    pub(super) target_repository_id: i64,
    pub(super) session_id: &'a str,
    pub(super) demand_id: u64,
    pub(super) message_id: Option<i64>,
    pub(super) observed_ms: u128,
    pub(super) assigned_jobs: u32,
    pub(super) running_jobs: u32,
    pub(super) ordinal: u64,
}

/// Request identity persisted with an Available slot before any queue effect.
pub(super) struct AvailableLaunchIdentity<'a> {
    pub(super) message: Option<i64>,
    pub(super) request: Option<i64>,
    pub(super) workflow_run: Option<i64>,
    pub(super) job: Option<&'a str>,
}

/// Reserve an Available offer before Acquire, JIT, or Docker effects.
pub(super) async fn reserve_available(
    journal: &Journal,
    route: ReplayRoute<'_>,
    session_id: &str,
    binding: &JournalDockerDaemonBinding,
    trust: &VerifiedJobTrust,
    maximum: NonZeroU32,
) -> Result<(ReserveOutcome, Option<ReservedLaunch>), HostError> {
    reserve_available_identity(
        journal,
        route,
        session_id,
        binding,
        trust.message_id(),
        trust.runner_request_id(),
        maximum,
    )
    .await
}

/// Reserve the journal key extracted from a verified offer. This lower-level
/// function is private to the Linux coordinator; production callers must only
/// pass identities taken from a non-forgeable GitHub `VerifiedJobTrust`.
async fn reserve_available_identity(
    journal: &Journal,
    route: ReplayRoute<'_>,
    session_id: &str,
    binding: &JournalDockerDaemonBinding,
    message_id: i64,
    request_id: i64,
    maximum: NonZeroU32,
) -> Result<(ReserveOutcome, Option<ReservedLaunch>), HostError> {
    let identity = ScopedLaunchIdentity::new(route, session_id, message_id, request_id)?;
    match journal
        .reserve_linux_launch_if_accepting(&identity, binding, maximum)
        .await?
    {
        BoundCapacityClaim::New(id) => Ok((ReserveOutcome::Reserved, Some(reserved(id)))),
        BoundCapacityClaim::Existing(_)
        | BoundCapacityClaim::ExistingUnbound(_)
        | BoundCapacityClaim::ExistingBindingChanged(_) => Ok((ReserveOutcome::Existing, None)),
        BoundCapacityClaim::Draining => Ok((ReserveOutcome::Draining, None)),
        BoundCapacityClaim::CapacityFull { .. } => Ok((ReserveOutcome::CapacityFull, None)),
    }
}

/// Reserve one journal key extracted from the non-clone assigned-demand
/// capability, projected only inside the Launch implementation.
pub(super) async fn reserve_assigned_identity(
    journal: &Journal,
    identity: AssignedSlotIdentity<'_>,
    binding: &JournalDockerDaemonBinding,
    maximum: NonZeroU32,
) -> Result<(ReserveOutcome, Option<ReservedLaunch>), HostError> {
    let observation = AssignedPopulationObservation::new(
        identity.observed_ms,
        identity.assigned_jobs,
        identity.running_jobs,
    )?;
    let identity = ScopedAssignedLaunchIdentity::new(
        identity.route,
        identity.target_repository_id,
        identity.session_id,
        identity.demand_id,
        identity.message_id,
        observation,
        identity.ordinal,
    )?;
    match journal
        .reserve_linux_assigned_launch_if_accepting(&identity, binding, maximum)
        .await?
    {
        BoundCapacityClaim::New(id) => Ok((ReserveOutcome::Reserved, Some(reserved(id)))),
        BoundCapacityClaim::Existing(_)
        | BoundCapacityClaim::ExistingUnbound(_)
        | BoundCapacityClaim::ExistingBindingChanged(_) => Ok((ReserveOutcome::Existing, None)),
        BoundCapacityClaim::Draining => Ok((ReserveOutcome::Draining, None)),
        BoundCapacityClaim::CapacityFull { .. } => Ok((ReserveOutcome::CapacityFull, None)),
    }
}

/// Commit identity/effect intent, perform one one-shot effect sequence, and
/// persist its terminal launch-row result. An effect error always holds capacity.
pub(super) async fn run_reserved_launch<F, Fut>(
    journal: &Journal,
    launch: &ReservedLaunch,
    message_id: Option<i64>,
    request_id: Option<i64>,
    workflow_run_id: Option<i64>,
    requested_job_id: Option<&str>,
    effect: F,
) -> Result<LaunchEffectOutcome, HostError>
where
    F: FnOnce(&str) -> Fut,
    Fut: Future<Output = Result<(), HostError>>,
{
    if journal
        .bind_launch_identity(
            launch.id,
            message_id,
            request_id,
            workflow_run_id,
            requested_job_id,
            &launch.runner_name,
        )
        .await
        .is_err()
    {
        return Ok(
            if journal.record_launch_no_effect(launch.id).await.is_ok() {
                LaunchEffectOutcome::NotDispatched
            } else {
                LaunchEffectOutcome::Uncertain
            },
        );
    }
    if journal
        .record_launch_effect_intent(launch.id)
        .await
        .is_err()
    {
        // If the effect marker did not commit, this releases only a fresh
        // scoped reservation with no identities. If the commit was ambiguous,
        // the journal predicate rejects this and leaves capacity held.
        return Ok(
            if journal.record_launch_no_effect(launch.id).await.is_ok() {
                LaunchEffectOutcome::NotDispatched
            } else {
                LaunchEffectOutcome::Uncertain
            },
        );
    }

    let outcome = match effect(&launch.runner_name).await {
        Ok(()) => velnor_runner_journal::journal::Outcome::Done,
        Err(_) => velnor_runner_journal::journal::Outcome::Uncertain,
    };
    journal.finish(launch.id, outcome).await?;
    Ok(match outcome {
        velnor_runner_journal::journal::Outcome::Done => LaunchEffectOutcome::Done,
        velnor_runner_journal::journal::Outcome::Uncertain
        | velnor_runner_journal::journal::Outcome::DefiniteFailure => {
            LaunchEffectOutcome::Uncertain
        }
    })
}

/// Run one verified session effect sequence under a durable global slot.
/// Acquire/JIT work is supplied by the caller and starts only after both the
/// launch identity and effect intent have committed. Pair staging then records
/// every exact Docker identity through the journal-owned `PairSink`.
pub(super) async fn run_reserved_worker<E, Jit, JitFuture, Gate, GateFuture>(
    engine: &E,
    journal: &Journal,
    launch: &ReservedLaunch,
    profile: &RunnerImageProfile,
    identity: AvailableLaunchIdentity<'_>,
    jit_for_runner: Jit,
    before_pair_start: Gate,
) -> Result<LaunchEffectOutcome, HostError>
where
    E: PairEngine,
    Jit: FnOnce(String) -> JitFuture,
    JitFuture: Future<Output = Result<EncodedJit, HostError>>,
    Gate: FnOnce() -> GateFuture,
    GateFuture: Future<Output = Result<(), HostError>>,
{
    let volume = match new_worker_volume() {
        Ok(volume) => volume,
        Err(_error) => {
            return Ok(
                if journal.record_launch_no_effect(launch.id).await.is_ok() {
                    LaunchEffectOutcome::NotDispatched
                } else {
                    LaunchEffectOutcome::Uncertain
                },
            );
        }
    };
    run_reserved_launch(
        journal,
        launch,
        identity.message,
        identity.request,
        identity.workflow_run,
        identity.job,
        |runner_name| {
            let runner_name = runner_name.to_owned();
            async move {
                let jit = jit_for_runner(runner_name).await?;
                before_pair_start().await?;
                start_worker_pair(
                    engine,
                    journal,
                    launch.id,
                    &volume,
                    profile,
                    jit.expose().as_bytes(),
                )
                .await?;
                Ok(())
            }
        },
    )
    .await
}

fn reserved(id: i64) -> ReservedLaunch {
    ReservedLaunch {
        id,
        runner_name: format!("v{id:x}"),
    }
}

#[cfg(test)]
#[path = "capacity_tests.rs"]
mod tests;
