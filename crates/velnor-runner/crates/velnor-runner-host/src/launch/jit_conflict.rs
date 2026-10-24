//! Recover a JIT-name conflict without starting or replaying the failed job.

mod local;
mod remote;

#[cfg(test)]
#[path = "jit_conflict/tests.rs"]
mod tests;

use bollard::Docker;

use crate::{
    config::RepositoryConnection,
    error::HostError,
    journal::{
        ConnectionJournal, EngineContext, JitConflictRecoveryClaim, Journal,
        MetadataSettlementResult,
    },
    launch::{
        ReleaseReadSecret, SelectedEngineContext, VerifiedWorkerImages,
        bootstrap::NativeCleanupRuntime,
    },
    listen::{Link, Secret},
};

/// Inputs bound to one prepared repository and execution environment.
pub(crate) struct ReconcileContext<'a> {
    pub(crate) docker: &'a Docker,
    pub(crate) journal: &'a Journal,
    pub(crate) scope: &'a ConnectionJournal,
    pub(crate) binding: &'a RepositoryConnection,
    pub(crate) set_id: i64,
    pub(crate) link: &'a mut Link,
    pub(crate) github_pat: &'a Secret,
    pub(crate) selected: &'a SelectedEngineContext,
    pub(crate) images: &'a VerifiedWorkerImages,
    pub(crate) release_secret: &'a std::sync::Arc<ReleaseReadSecret>,
}

/// Run local, remote, network, and metadata proofs under one current claim.
pub(crate) async fn reconcile_claim(
    context: ReconcileContext<'_>,
    claim: &JitConflictRecoveryClaim,
) -> Result<Option<MetadataSettlementResult>, HostError> {
    if claim.scale_set_id() != context.set_id
        || claim.requested_name() != format!("v{}", claim.physical_nonce())
        || context.scope.connection_id().as_str() != context.binding.connection_id.as_str()
        || claim.engine_id() != context.selected.engine_id()
        || claim.docker_root_digest() != context.selected.docker_root_digest()
    {
        return Err(HostError::Identity);
    }
    let engine = EngineContext {
        engine_id: context.selected.engine_id().to_owned(),
        endpoint: context.selected.endpoint().to_owned(),
        platform: context.selected.platform().to_owned(),
    };
    if !context.selected.matches_selection(&engine)
        || !context.selected.matches_images(context.images)
    {
        return Err(HostError::Ownership);
    }
    let runtime = NativeCleanupRuntime::new(
        context.docker.clone(),
        context.journal.clone(),
        engine,
        context.selected.clone(),
        context.images.clone(),
        std::sync::Arc::clone(context.release_secret),
    )?;
    local::ensure_local_absence(
        &runtime,
        context.docker,
        context.scope,
        context.selected,
        claim,
    )
    .await?;
    if !remote::reconcile(&context, claim).await? {
        return Ok(None);
    }
    ensure_network_absence(context.docker, context.scope, context.selected, claim).await?;
    let settled = runtime
        .settle_storage_metadata_after_cleanup(context.scope, claim.reservation_id())
        .await?;
    Ok(Some(settled))
}

async fn ensure_network_absence(
    docker: &Docker,
    scope: &ConnectionJournal,
    selected: &SelectedEngineContext,
    claim: &JitConflictRecoveryClaim,
) -> Result<(), HostError> {
    let identity = scope
        .storage_network_recovery_identity(claim.reservation_id())
        .await?
        .ok_or(HostError::Identity)?;
    if !matches!(
        identity.remove_state(),
        "removed" | "not_requested" | "requested" | "uncertain"
    ) {
        return Err(HostError::Identity);
    }
    if identity.remove_state() != "removed" {
        crate::launch::bootstrap::remove_owned_network_for_jit_conflict(
            docker,
            scope,
            selected,
            claim.reservation_id(),
            claim,
        )
        .await?;
    }
    scope.validate_jit_conflict_claim(claim).await
}
