use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::super::cleanup::{Context, EffectBudget, Failure, run_effect};
use super::RunnerLogAvailability;
use super::runner_log_state::{CaptureBinding, CaptureState, CaptureStateRequest};
use super::runner_logs::RunnerLogStageRequest;
use super::stage::{OperationKey, StageRequest};
use super::{CAPTURE_DEADLINE, DiagnosticsRoot, worker};
use crate::error::HostError;
use crate::journal::{CleanupClaim, CompletedLaunch};

#[path = "capture/native.rs"]
mod native;

pub(in crate::launch) use native::{NativeCaptureRequest, capture_native, delete_after_capture};

pub(in crate::launch) async fn persist_runner_log_projection(
    context: &Context<'_>,
    root: &DiagnosticsRoot,
    launch: &CompletedLaunch,
    claim: CleanupClaim,
    budget: &mut EffectBudget<'_>,
    binding: &CaptureBinding,
    terminal: &CaptureState,
    attempt: i64,
    archive: &[u8],
    started: Instant,
) -> Result<CaptureState, Failure> {
    let write = RunnerLogStageRequest::new(
        root,
        OperationKey {
            journal_identity: binding.journal_identity.clone(),
            connection_id: binding.connection_id.clone(),
            intent_id: binding.intent_id,
        },
        attempt,
        &binding.runner_container_id,
        archive,
        started,
    )
    .map_err(|error| Failure::request("runner log projection", error))?;
    let availability = match write.availability() {
        super::runner_logs::RunnerLogAvailability::None => RunnerLogAvailability::None,
        super::runner_logs::RunnerLogAvailability::RunnerOnly => RunnerLogAvailability::RunnerOnly,
        super::runner_logs::RunnerLogAvailability::WorkerOnly => RunnerLogAvailability::WorkerOnly,
        super::runner_logs::RunnerLogAvailability::RunnerAndWorker => {
            RunnerLogAvailability::RunnerAndWorker
        }
    };
    let (digest, size) = write.projection_identity();
    let pending = terminal
        .projection_pending(digest, size, availability != RunnerLogAvailability::None)
        .map_err(|error| Failure::request("runner log pending state", error))?;
    persist_runner_log_state(
        context, root, launch, claim, budget, binding, &pending, started,
    )
    .await?;
    persist_durable_write(
        context,
        launch,
        claim,
        budget,
        DurableWrite::Projection(write),
        started,
    )
    .await?;
    pending
        .projected()
        .map_err(|error| Failure::request("runner log complete state", error))
}

pub(in crate::launch) async fn persist_runner_log_state(
    context: &Context<'_>,
    root: &DiagnosticsRoot,
    launch: &CompletedLaunch,
    claim: CleanupClaim,
    budget: &mut EffectBudget<'_>,
    binding: &CaptureBinding,
    state: &CaptureState,
    started: Instant,
) -> Result<(), Failure> {
    let request = CaptureStateRequest::new(root, binding, state, started)
        .map_err(|error| Failure::request("runner log state", error))?;
    persist_durable_write(
        context,
        launch,
        claim,
        budget,
        DurableWrite::State(request),
        started,
    )
    .await
}

enum DurableWrite {
    Projection(RunnerLogStageRequest),
    State(CaptureStateRequest),
}

async fn persist_durable_write(
    context: &Context<'_>,
    launch: &CompletedLaunch,
    claim: CleanupClaim,
    budget: &mut EffectBudget<'_>,
    write: DurableWrite,
    started: Instant,
) -> Result<(), Failure> {
    let remaining = CAPTURE_DEADLINE.saturating_sub(started.elapsed());
    if remaining.is_zero() {
        return Err(Failure::request(
            "runner log write",
            HostError::DockerTimeout,
        ));
    }
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_write = run_effect(
        context.journal,
        budget,
        launch.intent.id,
        claim,
        "runner log write",
        || async {
            let worker = worker::process_worker()?;
            let receiver = match write {
                DurableWrite::Projection(request) => {
                    worker.submit_runner_logs(request, Arc::clone(&cancelled))?
                }
                DurableWrite::State(request) => {
                    worker.submit_capture_state(request, Arc::clone(&cancelled))?
                }
            };
            match tokio::time::timeout(remaining, receiver).await {
                Ok(Ok(result)) => result,
                Ok(Err(_)) => Err(HostError::Journal),
                Err(_) => {
                    cancelled.store(true, Ordering::Release);
                    Err(HostError::DockerTimeout)
                }
            }
        },
    )
    .await;
    if let Err(error) = worker_write {
        cancelled.store(true, Ordering::Release);
        return Err(error);
    }
    if !context
        .journal
        .completion_cleanup_claim_current(launch.intent.id, claim.generation)
        .await
        .map_err(|error| Failure::request("runner log claim check", error))?
    {
        return Err(Failure::claim_lost());
    }
    Ok(())
}

fn stage_request(
    root: &DiagnosticsRoot,
    launch: &CompletedLaunch,
    claim: CleanupClaim,
    role: &str,
    container_id: &str,
    bytes: Vec<u8>,
    started: Instant,
) -> Result<StageRequest, Failure> {
    let state = root
        .state
        .try_clone()
        .map_err(|_| Failure::request("diagnostic root", HostError::Path))?;
    let parent = root
        .parent
        .try_clone()
        .map_err(|_| Failure::request("diagnostic parent", HostError::Path))?;
    let scope = root
        .scope
        .try_clone()
        .map_err(|_| Failure::request("diagnostic scope", HostError::Path))?;
    Ok(StageRequest {
        state,
        parent,
        scope,
        key: OperationKey {
            journal_identity: root.journal_identity.clone(),
            connection_id: root.connection_id.clone(),
            intent_id: launch.intent.id,
        },
        generation: claim.generation,
        role: role.to_owned(),
        container_id: container_id.to_owned(),
        bytes,
        started,
    })
}

struct DispatchRequest<'a, 'b> {
    launch: &'a CompletedLaunch,
    claim: CleanupClaim,
    budget: &'a mut EffectBudget<'b>,
    #[cfg(test)]
    root: &'a DiagnosticsRoot,
    stage_request: StageRequest,
    remaining: Duration,
    cancelled: Arc<AtomicBool>,
}

async fn dispatch_stage(
    context: &Context<'_>,
    request: DispatchRequest<'_, '_>,
) -> Result<(), Failure> {
    let DispatchRequest {
        launch,
        claim,
        budget,
        #[cfg(test)]
        root,
        stage_request,
        remaining,
        cancelled,
    } = request;
    #[cfg(test)]
    let next_write_gate = root
        .take_next_write_gate()
        .map_err(|error| Failure::request("diagnostic worker gate", error))?;
    let operation_cancelled = Arc::clone(&cancelled);
    run_effect(
        context.journal,
        budget,
        launch.intent.id,
        claim,
        "diagnostic write",
        || async move {
            let worker = worker::process_worker()?;
            #[cfg(test)]
            let reply = if let Some(gate) = next_write_gate {
                worker.submit_gated(stage_request, operation_cancelled.clone(), gate)?
            } else {
                worker.submit(stage_request, operation_cancelled.clone())?
            };
            #[cfg(not(test))]
            let reply = worker.submit(stage_request, operation_cancelled.clone())?;
            match tokio::time::timeout(remaining, reply).await {
                Ok(Ok(result)) => result,
                Ok(Err(_)) => Err(HostError::Journal),
                Err(_) => {
                    operation_cancelled.store(true, Ordering::Release);
                    Err(HostError::DockerTimeout)
                }
            }
        },
    )
    .await
}
