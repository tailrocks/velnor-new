use std::sync::Arc;

use crate::{
    error::HostError,
    journal::{ConnectionJournal, Journal},
    launch::{SelectedEngineContext, bootstrap::cleanup::NativeCleanupRuntime},
};

use super::docker::StatefulDocker;

pub(crate) struct CleanupRuntimeFixture {
    pub(crate) base: crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture,
    pub(crate) runtime: NativeCleanupRuntime,
    pub(crate) server: StatefulDocker,
    pub(crate) scope: ConnectionJournal,
    pub(crate) journal: Journal,
    pub(crate) reservation_id: i64,
}

pub(crate) async fn cleanup_runtime_fixture() -> Result<CleanupRuntimeFixture, HostError> {
    let base = crate::journal::request_lifecycle::tests::native_started_intake_fixture().await?;
    cleanup_runtime_fixture_for_base(base).await
}

pub(crate) async fn cleanup_runtime_fixture_for_base(
    base: crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture,
) -> Result<CleanupRuntimeFixture, HostError> {
    cleanup_runtime_fixture_for_base_with_capacity(
        base,
        0,
        19 * 1024 * 1024 * 1024,
        40 * 1024 * 1024 * 1024,
    )
    .await
}

pub(crate) async fn cleanup_runtime_fixture_for_base_with_capacity(
    base: crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture,
    capacity_first: u64,
    capacity_after_first: u64,
    capacity_total: u64,
) -> Result<CleanupRuntimeFixture, HostError> {
    let reservation_id = base.reservation_id;
    let (runtime, server) = cleanup_runtime_parts(
        &base.scope,
        &base.journal,
        reservation_id,
        capacity_first,
        capacity_after_first,
        capacity_total,
    )
    .await?;
    Ok(CleanupRuntimeFixture {
        scope: base.scope.clone(),
        journal: base.journal.clone(),
        base,
        runtime,
        server,
        reservation_id,
    })
}

pub(crate) async fn verify_storage_cleanup_for_scope(
    scope: &ConnectionJournal,
    journal: &Journal,
    reservation_id: i64,
) -> Result<crate::launch::bootstrap::VerifiedStorageCleanup, HostError> {
    let (runtime, server) = cleanup_runtime_parts(
        scope,
        journal,
        reservation_id,
        0,
        19 * 1024 * 1024 * 1024,
        40 * 1024 * 1024 * 1024,
    )
    .await?;
    let pending = runtime
        .prepare_storage_cleanup(scope, reservation_id)
        .await?;
    let proof = runtime.verify_storage_cleanup(scope, pending).await?;
    drop(server);
    Ok(proof)
}

async fn cleanup_runtime_parts(
    scope: &ConnectionJournal,
    journal: &Journal,
    reservation_id: i64,
    capacity_first: u64,
    capacity_after_first: u64,
    capacity_total: u64,
) -> Result<(NativeCleanupRuntime, StatefulDocker), HostError> {
    let identity = scope
        .storage_cleanup_identity(reservation_id)
        .await?
        .ok_or(HostError::Ownership)?;
    let engine_context = scope.engine_context().await?.ok_or(HostError::Ownership)?;
    let images = crate::launch::verified_images::test_worker_images_for(
        identity.engine_id(),
        identity.docker_root_digest(),
        "x86_64",
        "linux/amd64",
    );
    let selected = SelectedEngineContext::bind(&engine_context, &images)?;
    let provider = crate::launch::resource_probe::CleanupObserverTestProvider::new(&selected);
    let probe_config = serde_json::to_value(provider.config()).map_err(|_| HostError::Frame)?;
    let server = StatefulDocker::open_with_capacity(
        &images,
        identity,
        probe_config,
        capacity_first,
        capacity_after_first,
        capacity_total,
    )?;
    let docker = server.docker();
    let runtime = NativeCleanupRuntime::new_with_test_provider(
        docker,
        journal.clone(),
        engine_context,
        selected,
        images,
        Arc::new(crate::launch::resource_probe::ReleaseReadSecret::test_inert()),
        provider,
    )?;
    Ok((runtime, server))
}

#[test]
fn native_cleanup_runtime_uses_persisted_witness_and_real_observer_lifecycle() -> Result<(), String>
{
    crate::launch::test_harness_stack::run_on_test_harness_stack(
        "native_cleanup_runtime_uses_persisted_witness_and_real_observer_lifecycle",
        || {
            Box::pin(async {
                native_cleanup_runtime_uses_persisted_witness_and_real_observer_lifecycle_on_fixture_stack().await.map_err(|error| error.to_string())
            })
        },
    )
}

async fn native_cleanup_runtime_uses_persisted_witness_and_real_observer_lifecycle_on_fixture_stack()
-> Result<(), HostError> {
    let fixture = cleanup_runtime_fixture().await?;
    let pending = fixture
        .runtime
        .prepare_storage_cleanup(&fixture.scope, fixture.reservation_id)
        .await?;
    let proof = fixture
        .runtime
        .verify_storage_cleanup(&fixture.scope, pending)
        .await?;
    assert_eq!(proof.reservation_id(), fixture.reservation_id);
    assert!(proof.all_owned_refs_absent());
    Ok(())
}
