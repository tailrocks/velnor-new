use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use bollard::models::ContainerInspectResponse;

use super::super::cleanup::{Context, EffectBudget, Failure, run_effect};
use super::stage::{OperationKey, StageRequest};
use super::{
    CAPTURE_DEADLINE, DiagnosticRecord, DiagnosticsRoot, INSPECT_DEADLINE, MAX_RECORD_BYTES,
    RecordIdentity, diagnostic_record_fields, worker,
};
use crate::error::HostError;
use crate::journal::{CleanupClaim, CompletedLaunch, StorageCleanupIdentity};

pub(in crate::launch) struct NativeCaptureRequest<'a, 'b> {
    pub(in crate::launch) root: &'a DiagnosticsRoot,
    pub(in crate::launch) launch: &'a CompletedLaunch,
    pub(in crate::launch) claim: CleanupClaim,
    pub(in crate::launch) budget: &'a mut EffectBudget<'b>,
    pub(in crate::launch) role: &'static str,
    pub(in crate::launch) expected_name: &'a str,
    pub(in crate::launch) container_id: &'a str,
    pub(in crate::launch) identity: &'a StorageCleanupIdentity,
}

type NativeIdentityValidator = fn(
    &ContainerInspectResponse,
    &str,
    &str,
    &str,
    &StorageCleanupIdentity,
    &str,
) -> Result<(), HostError>;

pub(in crate::launch) async fn capture_native(
    context: &Context<'_>,
    request: NativeCaptureRequest<'_, '_>,
    validate: NativeIdentityValidator,
) -> Result<(), Failure> {
    let NativeCaptureRequest {
        root,
        launch,
        claim,
        budget,
        role,
        expected_name,
        container_id,
        identity,
    } = request;
    let started = Instant::now();
    let response = inspect_container(context, budget, launch, claim, container_id, started).await?;
    validate(
        &response,
        container_id,
        expected_name,
        role,
        identity,
        &root.connection_id,
    )
    .map_err(|error| Failure::request("container diagnostic inspect", error))?;
    let record = diagnostic_record_fields(
        response,
        &RecordIdentity {
            connection_id: root.connection_id.clone(),
            intent_id: launch.intent.id,
            generation: claim.generation,
            role,
            expected_name,
            container_id,
        },
    )
    .map_err(|error| Failure::request("container diagnostic inspect", error))?;
    if !context
        .journal
        .completion_cleanup_claim_current(launch.intent.id, claim.generation)
        .await
        .map_err(|error| Failure::request("diagnostic claim check", error))?
    {
        return Err(Failure::claim_lost());
    }
    persist_capture(
        context,
        PersistRequest {
            root,
            launch,
            claim,
            budget,
            role,
            container_id,
            record,
            started,
        },
    )
    .await
}

async fn inspect_container(
    context: &Context<'_>,
    budget: &mut EffectBudget<'_>,
    launch: &CompletedLaunch,
    claim: CleanupClaim,
    container_id: &str,
    started: Instant,
) -> Result<ContainerInspectResponse, Failure> {
    let docker = context.docker;
    run_effect(
        context.journal,
        budget,
        launch.intent.id,
        claim,
        "container diagnostic inspect",
        || async move {
            let inspect_limit = CAPTURE_DEADLINE
                .saturating_sub(started.elapsed())
                .min(INSPECT_DEADLINE);
            if inspect_limit.is_zero() {
                return Err(HostError::DockerTimeout);
            }
            tokio::time::timeout(inspect_limit, docker.inspect_container(container_id, None))
                .await
                .map_err(|_| HostError::DockerTimeout)?
                .map_err(|_| HostError::Docker)
        },
    )
    .await
}

struct PersistRequest<'a, 'b> {
    root: &'a DiagnosticsRoot,
    launch: &'a CompletedLaunch,
    claim: CleanupClaim,
    budget: &'a mut EffectBudget<'b>,
    role: &'static str,
    container_id: &'a str,
    record: DiagnosticRecord,
    started: Instant,
}

async fn persist_capture(
    context: &Context<'_>,
    request: PersistRequest<'_, '_>,
) -> Result<(), Failure> {
    let PersistRequest {
        root,
        launch,
        claim,
        budget,
        role,
        container_id,
        record,
        started,
    } = request;
    let bytes = serde_json::to_vec(&record)
        .map_err(|_| Failure::request("diagnostic record", HostError::Journal))?;
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(Failure::request("diagnostic record", HostError::Ownership));
    }
    let remaining = CAPTURE_DEADLINE.saturating_sub(started.elapsed());
    if remaining.is_zero() {
        return Err(Failure::request(
            "diagnostic write",
            HostError::DockerTimeout,
        ));
    }
    let stage_request = stage_request(root, launch, claim, role, container_id, bytes, started)?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let write = dispatch_stage(
        context,
        DispatchRequest {
            launch,
            claim,
            budget,
            #[cfg(test)]
            root,
            stage_request,
            remaining,
            cancelled: Arc::clone(&cancelled),
        },
    )
    .await;
    if let Err(failure) = write {
        cancelled.store(true, Ordering::Release);
        return Err(failure);
    }
    if !context
        .journal
        .completion_cleanup_claim_current(launch.intent.id, claim.generation)
        .await
        .map_err(|error| Failure::request("diagnostic claim check", error))?
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

pub(in crate::launch) async fn delete_after_capture<D>(
    capture: Result<(), Failure>,
    delete: D,
) -> Result<(), Failure>
where
    D: std::future::Future<Output = Result<(), Failure>>,
{
    capture?;
    delete.await
}
