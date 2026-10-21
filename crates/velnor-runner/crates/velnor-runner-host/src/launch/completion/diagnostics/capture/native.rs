use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use bollard::models::ContainerInspectResponse;

use super::super::super::cleanup::{Context, EffectBudget, Failure, run_effect};
use super::super::{
    CAPTURE_DEADLINE, DiagnosticRecord, DiagnosticsRoot, INSPECT_DEADLINE, MAX_RECORD_BYTES,
    RecordIdentity, diagnostic_record_fields,
};
use super::{DispatchRequest, dispatch_stage, stage_request};
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
