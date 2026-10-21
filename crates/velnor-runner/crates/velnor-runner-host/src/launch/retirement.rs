//! Run post-JIT retirement operations with the configured repository credentials.

mod remote_absence;

#[cfg(test)]
mod tests;

use std::time::{Duration, Instant};

use bollard::Docker;

use crate::{
    config::RepositoryConnection,
    error::HostError,
    journal::{
        ConnectionJournal, EngineContext, Journal, MetadataSettlementResult, PostJitRetirementClaim,
    },
    launch::{SelectedEngineContext, VerifiedWorkerImages, bootstrap::NativeCleanupRuntime},
};

pub(crate) use remote_absence::{
    AuthenticatedRetirementAbsence, observe_retirement_runner_absence,
};

pub(crate) struct RetirementContext<'a> {
    pub(crate) docker: &'a Docker,
    pub(crate) journal: &'a Journal,
    pub(crate) scope: &'a ConnectionJournal,
    pub(crate) binding: &'a RepositoryConnection,
    pub(crate) selected: &'a SelectedEngineContext,
    pub(crate) images: &'a VerifiedWorkerImages,
    pub(crate) release_secret: &'a std::sync::Arc<crate::launch::ReleaseReadSecret>,
    pub(crate) claim: &'a PostJitRetirementClaim,
}

/// Recover one claimed physical attempt. Check remote and local absence before releasing capacity.
pub(crate) async fn reconcile_claim(
    context: RetirementContext<'_>,
) -> Result<MetadataSettlementResult, HostError> {
    let RetirementContext {
        docker,
        journal,
        scope,
        binding,
        selected,
        images,
        release_secret,
        claim,
    } = context;
    if claim.reservation_id() <= 0 || claim.scale_set_id() <= 0 {
        return Err(HostError::Identity);
    }
    let engine = EngineContext {
        engine_id: selected.engine_id().to_owned(),
        endpoint: selected.endpoint().to_owned(),
        platform: selected.platform().to_owned(),
    };
    let runtime = NativeCleanupRuntime::new(
        docker.clone(),
        journal.clone(),
        engine,
        selected.clone(),
        images.clone(),
        std::sync::Arc::clone(release_secret),
    )?;
    Box::pin(reconcile_claim_effects(
        &runtime, docker, scope, binding, selected, claim,
    ))
    .await
}

async fn reconcile_claim_effects(
    runtime: &NativeCleanupRuntime,
    docker: &Docker,
    scope: &ConnectionJournal,
    binding: &RepositoryConnection,
    selected: &SelectedEngineContext,
    claim: &PostJitRetirementClaim,
) -> Result<MetadataSettlementResult, HostError> {
    if !scope.resume_retirement_local_absence(claim).await? {
        let pending = runtime
            .prepare_storage_cleanup(scope, claim.reservation_id())
            .await?;
        Box::pin(crate::launch::remove_owned_resources_for_retirement(
            docker, scope, claim, selected, &pending,
        ))
        .await?;
        let local = runtime.verify_storage_cleanup(scope, pending).await?;
        scope.record_retirement_local_absence(claim, local).await?;
    }
    let deadline = Instant::now() + Duration::from_secs(45);
    let remote = observe_retirement_runner_absence(scope, claim, binding, deadline)
        .await
        .map_err(|_| HostError::Journal)?;
    finish_after_remote(runtime, docker, scope, selected, claim, &remote).await
}

async fn finish_after_remote(
    runtime: &NativeCleanupRuntime,
    docker: &Docker,
    scope: &ConnectionJournal,
    selected: &SelectedEngineContext,
    claim: &PostJitRetirementClaim,
    remote: &AuthenticatedRetirementAbsence,
) -> Result<MetadataSettlementResult, HostError> {
    scope
        .record_retirement_remote_absence(claim, remote)
        .await
        .map_err(|error| stage_error("remote receipt", error))?;
    scope
        .record_retirement_storage_phase(claim)
        .await
        .map_err(|error| stage_error("storage phase", error))?;
    if !scope.resume_retirement_network_absence(claim).await? {
        crate::launch::bootstrap::remove_owned_network_for_retirement(
            docker,
            scope,
            selected,
            claim.reservation_id(),
            claim,
        )
        .await
        .map_err(|error| stage_error("network removal", error))?;
    }
    runtime
        .settle_storage_metadata_after_cleanup(scope, claim.reservation_id())
        .await
        .map_err(|error| stage_error("capacity settlement", error))
}

fn stage_error(stage: &str, error: HostError) -> HostError {
    #[cfg(test)]
    eprintln!("retirement stage failed: {stage} ({error:?})");
    #[cfg(not(test))]
    let _ = stage;
    error
}
