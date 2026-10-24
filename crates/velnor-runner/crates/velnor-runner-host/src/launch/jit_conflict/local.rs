use bollard::Docker;

use crate::{
    error::HostError,
    journal::{ConnectionJournal, JitConflictRecoveryClaim},
    launch::{
        SelectedEngineContext, bootstrap::NativeCleanupRuntime,
        remove_owned_resources_for_jit_conflict,
    },
};

pub(super) async fn ensure_local_absence(
    runtime: &NativeCleanupRuntime,
    docker: &Docker,
    scope: &ConnectionJournal,
    selected: &SelectedEngineContext,
    claim: &JitConflictRecoveryClaim,
) -> Result<(), HostError> {
    if scope.resume_jit_conflict_local_absence(claim).await? {
        return Ok(());
    }
    let pending = runtime
        .prepare_storage_cleanup(scope, claim.reservation_id())
        .await?;
    remove_owned_resources_for_jit_conflict(docker, scope, claim, selected, &pending).await?;
    let proof = runtime.verify_storage_cleanup(scope, pending).await?;
    scope.record_jit_conflict_local_absence(claim, proof).await
}
