#[path = "retirement_tests/docker.rs"]
mod docker;
use docker::{DeleteMode, WorkerDocker};
use std::sync::atomic::Ordering;

use crate::{
    daemon_lock::{DaemonLock, EngineLineageGuard, canonical_journal_path},
    error::HostError,
    journal::{ConnectionJournal, Journal, PostJitRetirementClaim},
    launch::{
        SelectedEngineContext, SessionRefreshStartupAuthority,
        bootstrap::PendingStorageCleanupWitness,
    },
};

#[test]
fn rejected_container_delete_keeps_retirement_unadvanced() -> Result<(), String> {
    crate::launch::test_harness_stack::run_on_test_harness_stack(
        "retirement-rejected-container-delete",
        || Box::pin(rejected_container_delete_keeps_retirement_unadvanced_inner()),
    )
}

async fn rejected_container_delete_keeps_retirement_unadvanced_inner() -> Result<(), String> {
    Box::pin(rejected_container_delete_keeps_retirement_unadvanced_case())
        .await
        .map_err(|error| error.to_string())
}

async fn rejected_container_delete_keeps_retirement_unadvanced_case() -> Result<(), HostError> {
    let fixture = Box::pin(prepared_retirement()).await?;
    let server = WorkerDocker::open(
        &fixture.pending,
        fixture.scope.connection_id().as_str(),
        DeleteMode::Reject,
    )?;
    let result = Box::pin(super::remove_owned_resources_for_retirement(
        &server.docker,
        &fixture.scope,
        &fixture.claim,
        &fixture.selected,
        &fixture.pending,
    ))
    .await;

    assert_eq!(result, Err(HostError::Docker));
    assert_eq!(server.delete_count.load(Ordering::Relaxed), 1);
    assert_eq!(server.inspect_counts[0].load(Ordering::Relaxed), 1);
    assert_eq!(
        retirement_phase_count(&fixture.journal, &fixture.claim).await?,
        1
    );
    Ok(())
}

#[test]
fn surviving_container_after_delete_is_rejected_by_reinspection() -> Result<(), String> {
    crate::launch::test_harness_stack::run_on_test_harness_stack(
        "retirement-surviving-container-reinspection",
        || Box::pin(surviving_container_after_delete_is_rejected_by_reinspection_inner()),
    )
}

async fn surviving_container_after_delete_is_rejected_by_reinspection_inner() -> Result<(), String>
{
    Box::pin(surviving_container_after_delete_is_rejected_by_reinspection_case())
        .await
        .map_err(|error| error.to_string())
}

async fn surviving_container_after_delete_is_rejected_by_reinspection_case() -> Result<(), HostError>
{
    let fixture = Box::pin(prepared_retirement()).await?;
    let server = WorkerDocker::open(
        &fixture.pending,
        fixture.scope.connection_id().as_str(),
        DeleteMode::Keep,
    )?;
    let result = Box::pin(super::remove_owned_resources_for_retirement(
        &server.docker,
        &fixture.scope,
        &fixture.claim,
        &fixture.selected,
        &fixture.pending,
    ))
    .await;

    assert_eq!(result, Err(HostError::Ownership));
    assert_eq!(server.delete_count.load(Ordering::Relaxed), 1);
    assert_eq!(server.inspect_counts[0].load(Ordering::Relaxed), 2);
    assert_eq!(
        retirement_phase_count(&fixture.journal, &fixture.claim).await?,
        1
    );
    Ok(())
}

struct PreparedRetirement {
    scope: ConnectionJournal,
    journal: Journal,
    claim: PostJitRetirementClaim,
    selected: SelectedEngineContext,
    pending: PendingStorageCleanupWitness,
    _cleanup_fixture: crate::launch::bootstrap::CleanupRuntimeFixture,
}

async fn prepared_retirement() -> Result<PreparedRetirement, HostError> {
    let base = crate::journal::request_lifecycle::tests::native_started_intake_fixture().await?;
    recover_for_test(&base.journal, &base.database_path).await?;
    let claim = base
        .scope
        .claim_post_jit_retirement(base.reservation_id, 60)
        .await?
        .ok_or(HostError::Journal)?;
    let fixture = crate::launch::bootstrap::cleanup_runtime_fixture_for_base(base).await?;
    let pending = fixture
        .runtime
        .prepare_storage_cleanup(&fixture.scope, fixture.reservation_id)
        .await?;
    let engine_context = fixture
        .scope
        .engine_context()
        .await?
        .ok_or(HostError::Ownership)?;
    let images = crate::launch::verified_images::test_worker_images_for(
        pending.identity().engine_id(),
        pending.identity().docker_root_digest(),
        "x86_64",
        "linux/amd64",
    );
    let selected = SelectedEngineContext::bind(&engine_context, &images)?;
    Ok(PreparedRetirement {
        scope: fixture.scope.clone(),
        journal: fixture.journal.clone(),
        claim,
        selected,
        pending,
        _cleanup_fixture: fixture,
    })
}

async fn retirement_phase_count(
    journal: &Journal,
    claim: &PostJitRetirementClaim,
) -> Result<i64, HostError> {
    let connection = journal.connection().await?;
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM physical_attempt_post_jit_retirement_progress WHERE reservation_id=?1 AND claim_generation=?2",
            (claim.reservation_id(), claim.claim_generation()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)
}

async fn recover_for_test(
    journal: &Journal,
    database_path: &std::path::Path,
) -> Result<(), HostError> {
    let canonical_path = canonical_journal_path(database_path)?;
    let state_dir = database_path.parent().ok_or(HostError::Path)?;
    let lock_path = state_dir.join("daemon.lock");
    let lock = DaemonLock::try_acquire(&lock_path)?;
    let engine_id = journal.engine_id().await?;
    let guard = EngineLineageGuard::acquire_at_for_tests(&engine_id, state_dir)?;
    let instance_id = journal.instance_id().await?;
    let revision = journal.revision().await?;
    guard.verify_lineage(&canonical_path, &instance_id, revision)?;
    journal.attach_lineage_guard(guard.clone())?;
    journal.bootstrap_header_authority().await?;
    let instance_id = journal.instance_id().await?;
    let revision = journal.revision().await?;
    let authority = SessionRefreshStartupAuthority::test_only(
        &lock,
        &lock_path,
        &canonical_path,
        &instance_id,
        &engine_id,
        revision,
        &guard,
    )?;
    journal.recover_interrupted_host_work(authority).await
}
