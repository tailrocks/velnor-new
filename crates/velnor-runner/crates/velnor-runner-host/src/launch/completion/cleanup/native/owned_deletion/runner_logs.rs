use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use bollard::models::ExecInspectResponse;

use crate::{
    error::HostError,
    journal::{CleanupClaim, CompletedLaunch, StorageCleanupIdentity, StorageRole},
    launch::completion::diagnostics,
    worker::{WorkerContainerRole, WorkerIdentityNames},
};

use super::super::super::super::{Context, EffectBudget, Failure};
use super::{ContainerDeletion, request_effect, within};

const TOTAL_CAPTURE_LIMIT: Duration = Duration::from_secs(60);
const EXIT_GRACE: Duration = Duration::from_secs(2);
const INSPECT_INTERVAL: Duration = Duration::from_secs(1);
const MAX_CAPTURE_ATTEMPTS: i64 = 3;

#[path = "runner_logs/exec.rs"]
mod exec;
use exec::{capture_attempt, persist_state};

pub(super) async fn capture_if_needed(
    context: &Context<'_>,
    launch: &CompletedLaunch,
    claim: CleanupClaim,
    budget: &mut EffectBudget<'_>,
    target: &ContainerDeletion<'_>,
) -> Result<(), Failure> {
    let identity = target.identity;
    let binding = capture_binding(context.diagnostics, launch, identity)?;
    let state = match resume_capture(context, launch, claim, budget, &binding).await? {
        CaptureResume::Done => return Ok(()),
        CaptureResume::Capture(state) => state,
    };
    let started = Instant::now();
    let deadline = started
        .checked_add(TOTAL_CAPTURE_LIMIT)
        .ok_or_else(|| Failure::request("runner log deadline", HostError::DockerTimeout))?;
    let dind = inspected_dind(context, launch, claim, budget, identity).await?;
    validate_runner_home(&dind, identity)?;
    capture_attempts(
        context, launch, claim, budget, identity, &binding, state, started, deadline,
    )
    .await
}

enum CaptureResume {
    Done,
    Capture(diagnostics::CaptureState),
}

async fn resume_capture(
    context: &Context<'_>,
    launch: &CompletedLaunch,
    claim: CleanupClaim,
    budget: &mut EffectBudget<'_>,
    binding: &diagnostics::CaptureBinding,
) -> Result<CaptureResume, Failure> {
    let current = diagnostics::read_runner_log_state(context.diagnostics, binding)
        .map_err(|error| Failure::request("runner log state", error))?;
    let is_new = current.is_none();
    let mut state =
        current.unwrap_or_else(|| diagnostics::CaptureState::prepared(binding.clone(), 1));
    match state.phase {
        diagnostics::CapturePhase::Complete => {
            verify_saved_projection(context, binding, &state, true)?;
            return Ok(CaptureResume::Done);
        }
        diagnostics::CapturePhase::Unavailable => {
            verify_saved_projection(context, binding, &state, false)?;
            return Err(Failure::not_proven("runner logs unavailable"));
        }
        diagnostics::CapturePhase::ProjectionPending => {
            return resume_pending_projection(context, launch, claim, budget, binding, state).await;
        }
        diagnostics::CapturePhase::Terminal => {
            state = retry_capture(&state)?;
        }
        diagnostics::CapturePhase::Prepared
        | diagnostics::CapturePhase::Created
        | diagnostics::CapturePhase::StartRequested => {}
    }
    if is_new || state.phase == diagnostics::CapturePhase::Prepared {
        persist_state(context, launch, claim, budget, binding, &state).await?;
    }
    Ok(CaptureResume::Capture(state))
}

fn verify_saved_projection(
    context: &Context<'_>,
    binding: &diagnostics::CaptureBinding,
    state: &diagnostics::CaptureState,
    expected_complete: bool,
) -> Result<(), Failure> {
    diagnostics::read_runner_log_projection(context.diagnostics, binding, state)
        .map_err(|error| Failure::request("runner log replay", error))?;
    if (state.phase == diagnostics::CapturePhase::Complete) != expected_complete {
        return Err(Failure::not_proven("runner logs unavailable"));
    }
    Ok(())
}

async fn resume_pending_projection(
    context: &Context<'_>,
    launch: &CompletedLaunch,
    claim: CleanupClaim,
    budget: &mut EffectBudget<'_>,
    binding: &diagnostics::CaptureBinding,
    state: diagnostics::CaptureState,
) -> Result<CaptureResume, Failure> {
    if diagnostics::recover_runner_log_projection(context.diagnostics, binding, &state)
        .map_err(|error| Failure::request("runner log recovery", error))?
        .is_some()
    {
        let state = state
            .projected()
            .map_err(|error| Failure::request("runner log recovery", error))?;
        persist_state(context, launch, claim, budget, binding, &state).await?;
        return match state.phase {
            diagnostics::CapturePhase::Complete => Ok(CaptureResume::Done),
            diagnostics::CapturePhase::Unavailable => {
                Err(Failure::not_proven("runner logs unavailable"))
            }
            _ => Err(Failure::not_proven("runner log recovery state")),
        };
    }
    if context
        .diagnostics
        .pending_for(launch.intent.id)
        .map_err(|error| Failure::request("runner log worker state", error))?
    {
        return Err(Failure::not_proven("runner log write pending"));
    }
    let state = retry_capture(&state)?;
    persist_state(context, launch, claim, budget, binding, &state).await?;
    Ok(CaptureResume::Capture(state))
}

fn retry_capture(state: &diagnostics::CaptureState) -> Result<diagnostics::CaptureState, Failure> {
    state
        .retry_after_terminal()
        .map_err(|error| Failure::request("runner log retry", error))
}

async fn capture_attempts(
    context: &Context<'_>,
    launch: &CompletedLaunch,
    claim: CleanupClaim,
    budget: &mut EffectBudget<'_>,
    identity: &StorageCleanupIdentity,
    binding: &diagnostics::CaptureBinding,
    mut state: diagnostics::CaptureState,
    started: Instant,
    deadline: Instant,
) -> Result<(), Failure> {
    for _ in 0..MAX_CAPTURE_ATTEMPTS {
        if Instant::now() >= deadline {
            return Err(Failure::request(
                "runner log deadline",
                HostError::DockerTimeout,
            ));
        }
        let (terminal, archive) = capture_attempt(
            context, launch, claim, budget, identity, binding, state, started, deadline,
        )
        .await?;
        if let Some(archive) = archive {
            state = diagnostics::persist_runner_log_projection(
                context,
                context.diagnostics,
                launch,
                claim,
                budget,
                binding,
                &terminal,
                terminal.attempt,
                &archive,
                Instant::now(),
            )
            .await?;
            persist_state(context, launch, claim, budget, binding, &state).await?;
            return if state.phase == diagnostics::CapturePhase::Complete {
                Ok(())
            } else {
                Err(Failure::not_proven("runner logs unavailable"))
            };
        }
        if terminal.attempt >= MAX_CAPTURE_ATTEMPTS {
            return Err(Failure::not_proven("runner log output unavailable"));
        }
        state = terminal
            .retry_after_terminal()
            .map_err(|error| Failure::request("runner log retry", error))?;
        persist_state(context, launch, claim, budget, binding, &state).await?;
    }
    Err(Failure::not_proven("runner log capture attempts"))
}

fn capture_binding(
    root: &diagnostics::DiagnosticsRoot,
    launch: &CompletedLaunch,
    identity: &StorageCleanupIdentity,
) -> Result<diagnostics::CaptureBinding, Failure> {
    let home = identity
        .volumes()
        .iter()
        .find(|(role, _)| *role == StorageRole::RunnerHome)
        .map(|(_, name)| name.as_str())
        .ok_or_else(|| Failure::not_proven("runner home identity"))?;
    diagnostics::runner_log_binding(
        root,
        launch.intent.id,
        identity.reservation_id(),
        identity.operation_id(),
        identity.topology_version(),
        identity.engine_id(),
        identity.docker_root_digest(),
        identity.runner_container_id(),
        identity.dind_container_id(),
        home,
    )
    .map_err(|error| Failure::request("runner log identity", error))
}

async fn inspected_dind(
    context: &Context<'_>,
    launch: &CompletedLaunch,
    claim: CleanupClaim,
    budget: &mut EffectBudget<'_>,
    identity: &StorageCleanupIdentity,
) -> Result<bollard::models::ContainerInspectResponse, Failure> {
    let names = WorkerIdentityNames::from_private_volume(identity.private_volume())
        .map_err(|error| Failure::request("runner log DinD name", error))?;
    let name = names.container_name(WorkerContainerRole::Dind);
    let response = request_effect(
        context,
        launch,
        claim,
        budget,
        "runner log DinD inspect",
        identity,
        || async { super::inspect_container(context.docker, identity.dind_container_id()).await },
    )
    .await?
    .ok_or_else(|| Failure::not_proven("runner log DinD absent"))?;
    super::validate_container(
        &response,
        identity.dind_container_id(),
        &name,
        "dind",
        identity,
        context,
    )?;
    Ok(response)
}

fn validate_runner_home(
    dind: &bollard::models::ContainerInspectResponse,
    identity: &StorageCleanupIdentity,
) -> Result<(), Failure> {
    let expected = identity
        .volumes()
        .iter()
        .find(|(role, _)| *role == StorageRole::RunnerHome)
        .map(|(_, name)| name.as_str())
        .ok_or_else(|| Failure::not_proven("runner home identity"))?;
    let mounts = dind.mounts.as_deref().unwrap_or_default();
    let matches = mounts
        .iter()
        .filter(|mount| mount.destination.as_deref() == Some("/home/runner"))
        .collect::<Vec<_>>();
    if matches.len() != 1
        || matches[0].typ.as_deref() != Some("volume")
        || matches[0].name.as_deref() != Some(expected)
        || matches[0].rw != Some(true)
    {
        return Err(Failure::not_proven("runner home mount"));
    }
    if dind.state.as_ref().and_then(|state| state.running) != Some(true) {
        return Err(Failure::not_proven("runner log DinD not running"));
    }
    Ok(())
}

pub(super) async fn stop_and_confirm(
    context: &Context<'_>,
    launch: &CompletedLaunch,
    claim: CleanupClaim,
    budget: &mut EffectBudget<'_>,
    target: &ContainerDeletion<'_>,
    initially_running: bool,
) -> Result<(), Failure> {
    if initially_running {
        tokio::time::sleep(EXIT_GRACE).await;
    }
    let response = request_effect(
        context,
        launch,
        claim,
        budget,
        "runner quiescence inspect",
        target.identity,
        || async { super::inspect_container(context.docker, target.id).await },
    )
    .await?
    .ok_or_else(|| Failure::not_proven("runner absent before log capture"))?;
    super::validate_container(
        &response,
        target.id,
        target.name,
        target.role,
        target.identity,
        context,
    )?;
    if response.state.as_ref().and_then(|state| state.running) == Some(true) {
        request_effect(
            context,
            launch,
            claim,
            budget,
            "runner stop",
            target.identity,
            || async {
                super::within(context.docker.stop_container(
                    target.id,
                    Some(bollard::query_parameters::StopContainerOptions {
                        t: Some(10),
                        signal: None,
                    }),
                ))
                .await?
                .map_err(|_| HostError::Docker)
            },
        )
        .await?;
    }
    let stopped = request_effect(
        context,
        launch,
        claim,
        budget,
        "runner quiescence verify",
        target.identity,
        || async { super::inspect_container(context.docker, target.id).await },
    )
    .await?
    .ok_or_else(|| Failure::not_proven("runner absent before log capture"))?;
    super::validate_container(
        &stopped,
        target.id,
        target.name,
        target.role,
        target.identity,
        context,
    )?;
    if stopped.state.as_ref().and_then(|state| state.running) != Some(false) {
        return Err(Failure::not_proven("runner not quiescent"));
    }
    Ok(())
}
