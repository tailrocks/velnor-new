use std::collections::VecDeque;

use crate::{error::HostError, journal::MetadataSettlementResult};
use velnor_runner_github::{Exchange, Transport, TransportFail, ensure_runner_absent};

mod retry;

#[test]
fn orphan_retirement_holds_capacity_when_probe_is_insufficient() -> Result<(), String> {
    crate::launch::test_harness_stack::run_on_test_harness_stack(
        "orphan-retirement-insufficient-capacity",
        || Box::pin(orphan_retirement_case(0, 19 * GIB, 40 * GIB, false)),
    )
}

#[test]
fn orphan_retirement_settles_when_probe_meets_profile_reserve() -> Result<(), String> {
    crate::launch::test_harness_stack::run_on_test_harness_stack(
        "orphan-retirement-sufficient-capacity",
        || Box::pin(orphan_retirement_case(96 * GIB, 96 * GIB, 128 * GIB, true)),
    )
}

const GIB: u64 = 1024 * 1024 * 1024;

async fn orphan_retirement_case(
    capacity_first: u64,
    capacity_after_first: u64,
    capacity_total: u64,
    expect_settled: bool,
) -> Result<(), String> {
    Box::pin(orphan_retirement_case_inner(
        capacity_first,
        capacity_after_first,
        capacity_total,
        expect_settled,
    ))
    .await
    .map_err(|error| error.to_string())
}

async fn orphan_retirement_case_inner(
    capacity_first: u64,
    capacity_after_first: u64,
    capacity_total: u64,
    expect_settled: bool,
) -> Result<(), HostError> {
    let fixture = Box::pin(retirement_fixture(
        capacity_first,
        capacity_after_first,
        capacity_total,
    ))
    .await?;
    recover_fixture(base_fixture(&fixture))
        .await
        .inspect_err(|error| eprintln!("startup recovery fixture failed: {error:?}"))?;
    let claim = claim_orphan(&fixture.scope, fixture.reservation_id)
        .await
        .inspect_err(|error| eprintln!("orphan claim failed: {error:?}"))?;
    let selected = selected_engine(&fixture.scope, fixture.reservation_id)
        .await
        .inspect_err(|error| eprintln!("engine selection failed: {error:?}"))?;
    let remote = record_local_and_make_remote(&fixture, &claim, &selected).await?;
    let outcome = super::finish_after_remote(
        &fixture.runtime,
        &fixture.server.docker(),
        &fixture.scope,
        &selected,
        &claim,
        &remote,
    )
    .await
    .inspect_err(|error| eprintln!("remote finalization failed: {error:?}"))?;
    assert_eq!(
        outcome,
        if expect_settled {
            MetadataSettlementResult::Settled
        } else {
            MetadataSettlementResult::HeldAtCapacity
        }
    );
    verify_settlement(&fixture, expect_settled).await
}

async fn record_local_and_make_remote(
    fixture: &crate::launch::bootstrap::CleanupRuntimeFixture,
    claim: &crate::journal::PostJitRetirementClaim,
    selected: &crate::launch::SelectedEngineContext,
) -> Result<crate::launch::retirement::AuthenticatedRetirementAbsence, HostError> {
    let pending = fixture
        .runtime
        .prepare_storage_cleanup(&fixture.scope, fixture.reservation_id)
        .await
        .inspect_err(|error| eprintln!("cleanup preparation failed: {error:?}"))?;
    Box::pin(crate::launch::remove_owned_resources_for_retirement(
        &fixture.server.docker(),
        &fixture.scope,
        claim,
        selected,
        &pending,
    ))
    .await
    .inspect_err(|error| eprintln!("owned deletion failed: {error:?}"))?;
    let proof = fixture
        .runtime
        .verify_storage_cleanup(&fixture.scope, pending)
        .await
        .inspect_err(|error| eprintln!("cleanup observation failed: {error:?}"))?;
    fixture
        .scope
        .record_retirement_local_absence(claim, proof)
        .await
        .inspect_err(|error| eprintln!("local absence persistence failed: {error:?}"))?;
    let prepared = fixture
        .scope
        .prepare_retirement_remote_absence(claim)
        .await
        .inspect_err(|error| eprintln!("remote preparation failed: {error:?}"))?;
    let mut transport = EmptyRunnerTransport::new();
    let receipt = ensure_runner_absent(&mut transport, &base_fixture(fixture).runner, "test-token")
        .map_err(|_| HostError::Journal)?;
    crate::launch::retirement::remote_absence::test_authenticated_absence(
        prepared,
        receipt,
        101,
        "owner-101".to_owned(),
        "repo-101".to_owned(),
    )
}

async fn verify_settlement(
    fixture: &crate::launch::bootstrap::CleanupRuntimeFixture,
    expect_settled: bool,
) -> Result<(), HostError> {
    let live = fixture.scope.physical_runner_reservations().await?;
    let present = live
        .iter()
        .any(|row| row.reservation_id == fixture.reservation_id);
    let recoverable = fixture.scope.recoverable_post_jit_retirements().await?;
    if present == expect_settled || (expect_settled && !recoverable.is_empty()) {
        return Err(HostError::Identity);
    }
    Ok(())
}

fn base_fixture(
    fixture: &crate::launch::bootstrap::CleanupRuntimeFixture,
) -> &crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture {
    let crate::launch::bootstrap::CleanupRuntimeFixture { _base: base, .. } = fixture;
    base
}

async fn recover_fixture(
    fixture: &crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture,
) -> Result<(), HostError> {
    use crate::daemon_lock::{DaemonLock, EngineLineageGuard, canonical_journal_path};
    let path = canonical_journal_path(&fixture.database_path)?;
    let state_dir = fixture.database_path.parent().ok_or(HostError::Path)?;
    let lock_path = state_dir.join("daemon.lock");
    let daemon_lock = DaemonLock::try_acquire(&lock_path)?;
    let engine_id = fixture.journal.engine_id().await?;
    let guard = EngineLineageGuard::acquire_at_for_tests(&engine_id, state_dir)?;
    let instance_id = fixture.journal.instance_id().await?;
    let revision = fixture.journal.revision().await?;
    guard.verify_lineage(&path, &instance_id, revision)?;
    fixture.journal.attach_lineage_guard(guard.clone())?;
    fixture.journal.bootstrap_header_authority().await?;
    let instance_id = fixture.journal.instance_id().await?;
    let revision = fixture.journal.revision().await?;
    let authority = crate::launch::SessionRefreshStartupAuthority::test_only(
        &daemon_lock,
        &lock_path,
        &path,
        &instance_id,
        &engine_id,
        revision,
        &guard,
    )?;
    fixture
        .journal
        .recover_interrupted_host_work(authority)
        .await
}

async fn retirement_fixture(
    capacity_first: u64,
    capacity_after_first: u64,
    capacity_total: u64,
) -> Result<crate::launch::bootstrap::CleanupRuntimeFixture, HostError> {
    use crate::journal::request_lifecycle::tests::{
        helpers::TEST_ENGINE_ID, native_started_intake_fixture_with_filesystem_digest,
    };

    let sample_generation = format!("{:032x}", u32::from(b'a'));
    let permit =
        crate::launch::StartPermit::test_fixture_with_engine_id(&sample_generation, TEST_ENGINE_ID);
    let digest = crate::launch::resource_probe::filesystem_instance_digest(
        permit.engine_id(),
        permit.docker_root_digest(),
        b"12345678-1234-1234-1234-123456789abc",
        b"/dev/vdb",
        b"btrfs",
        0x9123_683e,
        0,
        38,
        0,
        41,
        17,
    );
    let base = native_started_intake_fixture_with_filesystem_digest(Some(digest)).await?;
    crate::launch::bootstrap::cleanup_runtime_fixture_for_base_with_capacity(
        base,
        capacity_first,
        capacity_after_first,
        capacity_total,
    )
    .await
}

async fn selected_engine(
    scope: &crate::journal::ConnectionJournal,
    reservation_id: i64,
) -> Result<crate::launch::SelectedEngineContext, HostError> {
    let identity = scope
        .storage_cleanup_identity(reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    let engine = scope.engine_context().await?.ok_or(HostError::Journal)?;
    let images = crate::launch::verified_images::test_worker_images_for(
        identity.engine_id(),
        identity.docker_root_digest(),
        "x86_64",
        "linux/amd64",
    );
    crate::launch::SelectedEngineContext::bind(&engine, &images)
}

async fn claim_orphan(
    scope: &crate::journal::ConnectionJournal,
    reservation_id: i64,
) -> Result<crate::journal::PostJitRetirementClaim, HostError> {
    claim_orphan_for(scope, reservation_id, 60).await
}

async fn claim_orphan_for(
    scope: &crate::journal::ConnectionJournal,
    reservation_id: i64,
    lease_seconds: i64,
) -> Result<crate::journal::PostJitRetirementClaim, HostError> {
    let candidates = scope.recoverable_post_jit_retirements().await?;
    let candidate = candidates
        .into_iter()
        .find(|candidate| candidate.reservation_id() == reservation_id)
        .ok_or(HostError::Journal)?;
    scope
        .claim_recoverable_post_jit_retirement(&candidate, lease_seconds)
        .await?
        .ok_or(HostError::Journal)
}

struct EmptyRunnerTransport {
    replies: VecDeque<(u16, Vec<u8>)>,
}

impl EmptyRunnerTransport {
    fn new() -> Self {
        Self {
            replies: VecDeque::from([(200, br#"{"count":0,"value":[]}"#.to_vec())]),
        }
    }
}

impl Transport for EmptyRunnerTransport {
    fn exchange(
        &mut self,
        _request: &velnor_runner_github::SessionRequest,
    ) -> Result<Exchange, TransportFail> {
        let (status, body) = self.replies.pop_front().ok_or(TransportFail::Reset)?;
        Ok(Exchange { status, body })
    }
}
