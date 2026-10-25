use std::{
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::{error::HostError, journal::MetadataSettlementResult};
use velnor_runner_github::{Exchange, Transport, TransportFail, ensure_runner_absent};

use super::{
    GIB, base_fixture, claim_orphan_for, recover_fixture, retirement_fixture, selected_engine,
};

const RETRY_CLAIM_LEASE_SECONDS: i64 = 30;

#[test]
fn capacity_hold_retries_with_retained_local_and_network_proofs() -> Result<(), String> {
    crate::launch::test_harness_stack::run_on_test_harness_stack(
        "orphan-retirement-capacity-retry",
        || Box::pin(capacity_hold_retry()),
    )
}

#[test]
fn remote_failure_retries_with_retained_local_proof() -> Result<(), String> {
    crate::launch::test_harness_stack::run_on_test_harness_stack(
        "orphan-retirement-remote-retry",
        || Box::pin(remote_failure_retry()),
    )
}

async fn capacity_hold_retry() -> Result<(), String> {
    let fixture = Box::pin(retirement_fixture(0, 96 * GIB, 128 * GIB))
        .await
        .map_err(show_error)?;
    recover_fixture(base_fixture(&fixture))
        .await
        .map_err(show_error)?;
    let first = claim_orphan_for(
        &fixture.scope,
        fixture.reservation_id,
        RETRY_CLAIM_LEASE_SECONDS,
    )
    .await
    .map_err(show_error)?;
    let selected = selected_engine(&fixture.scope, fixture.reservation_id)
        .await
        .map_err(show_error)?;
    Box::pin(record_local(&fixture, &first, &selected))
        .await
        .map_err(show_error)?;
    let remote = remote_receipt(&fixture, &first).await?;
    let held = super::super::finish_after_remote(
        &fixture.runtime,
        &fixture.server.docker(),
        &fixture.scope,
        &selected,
        &first,
        &remote,
    )
    .await
    .map_err(show_error)?;
    assert_eq!(held, MetadataSettlementResult::HeldAtCapacity);

    let second = expired_claim(&fixture.scope, fixture.reservation_id, &first).await?;
    assert!(
        fixture
            .scope
            .resume_retirement_local_absence(&second)
            .await
            .map_err(show_error)?
    );
    let remote = remote_receipt(&fixture, &second).await?;
    let settled = super::super::finish_after_remote(
        &fixture.runtime,
        &fixture.server.docker(),
        &fixture.scope,
        &selected,
        &second,
        &remote,
    )
    .await
    .map_err(show_error)?;
    assert_eq!(settled, MetadataSettlementResult::Settled);
    assert!(
        fixture
            .scope
            .physical_runner_reservations()
            .await
            .map_err(show_error)?
            .iter()
            .all(|row| row.reservation_id != fixture.reservation_id)
    );
    Ok(())
}

async fn remote_failure_retry() -> Result<(), String> {
    let fixture = Box::pin(retirement_fixture(96 * GIB, 96 * GIB, 128 * GIB))
        .await
        .map_err(show_error)?;
    recover_fixture(base_fixture(&fixture))
        .await
        .map_err(show_error)?;
    let first = claim_orphan_for(
        &fixture.scope,
        fixture.reservation_id,
        RETRY_CLAIM_LEASE_SECONDS,
    )
    .await
    .map_err(show_error)?;
    let selected = selected_engine(&fixture.scope, fixture.reservation_id)
        .await
        .map_err(show_error)?;
    Box::pin(record_local(&fixture, &first, &selected))
        .await
        .map_err(show_error)?;
    let _prepared = fixture
        .scope
        .prepare_retirement_remote_absence(&first)
        .await
        .map_err(show_error)?;
    assert!(
        ensure_runner_absent(
            &mut FailedRunnerTransport,
            &base_fixture(&fixture).runner,
            "test-token",
        )
        .is_err()
    );

    let second = expired_claim(&fixture.scope, fixture.reservation_id, &first).await?;
    assert!(
        fixture
            .scope
            .resume_retirement_local_absence(&second)
            .await
            .map_err(show_error)?
    );
    let remote = remote_receipt(&fixture, &second).await?;
    let settled = super::super::finish_after_remote(
        &fixture.runtime,
        &fixture.server.docker(),
        &fixture.scope,
        &selected,
        &second,
        &remote,
    )
    .await
    .map_err(show_error)?;
    assert_eq!(settled, MetadataSettlementResult::Settled);
    Ok(())
}

async fn record_local(
    fixture: &crate::launch::bootstrap::CleanupRuntimeFixture,
    claim: &crate::journal::PostJitRetirementClaim,
    selected: &crate::launch::SelectedEngineContext,
) -> Result<(), HostError> {
    let pending = fixture
        .runtime
        .prepare_storage_cleanup(&fixture.scope, fixture.reservation_id)
        .await?;
    Box::pin(crate::launch::remove_owned_resources_for_retirement(
        &fixture.server.docker(),
        &fixture.scope,
        claim,
        selected,
        &pending,
    ))
    .await?;
    let proof = fixture
        .runtime
        .verify_storage_cleanup(&fixture.scope, pending)
        .await?;
    fixture
        .scope
        .record_retirement_local_absence(claim, proof)
        .await
}

async fn remote_receipt(
    fixture: &crate::launch::bootstrap::CleanupRuntimeFixture,
    claim: &crate::journal::PostJitRetirementClaim,
) -> Result<crate::launch::retirement::AuthenticatedRetirementAbsence, String> {
    let prepared = fixture
        .scope
        .prepare_retirement_remote_absence(claim)
        .await
        .map_err(show_error)?;
    let receipt = ensure_runner_absent(
        &mut super::EmptyRunnerTransport::new(),
        &base_fixture(fixture).runner,
        "test-token",
    )
    .map_err(|error| format!("test runner absence failed: {error:?}"))?;
    crate::launch::retirement::remote_absence::test_authenticated_absence(
        prepared,
        receipt,
        101,
        "owner-101".to_owned(),
        "repo-101".to_owned(),
    )
    .map_err(show_error)
}

async fn expired_claim(
    scope: &crate::journal::ConnectionJournal,
    reservation_id: i64,
    previous: &crate::journal::PostJitRetirementClaim,
) -> Result<crate::journal::PostJitRetirementClaim, String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("The system time is before the Unix epoch. Error: {error}."))?
        .as_secs();
    let now = i64::try_from(now)
        .map_err(|error| format!("The Unix epoch time is invalid. Error: {error}."))?;
    let seconds_to_expiry = previous.lease_until().saturating_sub(now).max(0);
    let wait_seconds = u64::try_from(seconds_to_expiry)
        .map_err(|error| format!("The lease expiry delay is invalid. Error: {error}."))?
        .saturating_add(1);
    thread::sleep(Duration::from_secs(wait_seconds));
    let claim = claim_orphan_for(scope, reservation_id, RETRY_CLAIM_LEASE_SECONDS)
        .await
        .map_err(show_error)?;
    if claim.claim_generation() <= previous.claim_generation() {
        return Err("retirement claim generation did not advance".to_owned());
    }
    Ok(claim)
}

fn show_error(error: impl std::fmt::Debug) -> String {
    format!("{error:?}")
}

struct FailedRunnerTransport;

impl Transport for FailedRunnerTransport {
    fn exchange(
        &mut self,
        _request: &velnor_runner_github::SessionRequest,
    ) -> Result<Exchange, TransportFail> {
        Err(TransportFail::Reset)
    }
}
