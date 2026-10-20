use crate::{error::HostError, journal::MetadataSettlementResult};

use super::finish_after_settlement;

#[test]
fn settled_metadata_finishes_completion() {
    assert!(matches!(
        finish_after_settlement(MetadataSettlementResult::Settled),
        Ok(())
    ));
}

#[test]
fn capacity_hold_keeps_completion_retryable() {
    assert!(matches!(
        finish_after_settlement(MetadataSettlementResult::HeldAtCapacity),
        Err(super::super::Failure {
            phase: "metadata capacity hold",
            reason: "not_proven",
            error: HostError::Journal,
            stop_requested: false,
        })
    ));
}

#[test]
fn capacity_hold_retry_skips_proven_cleanup_effects() {
    assert_eq!(
        super::CleanupPhases::from_proofs(false, false, false, false),
        super::CleanupPhases {
            remote: true,
            storage: true,
            network: true,
            local: true,
        }
    );
    assert!(finish_after_settlement(MetadataSettlementResult::HeldAtCapacity).is_err());

    assert_eq!(
        super::CleanupPhases::from_proofs(true, true, true, true),
        super::CleanupPhases {
            remote: false,
            storage: false,
            network: false,
            local: false,
        }
    );
}

#[tokio::test]
async fn native_completion_reads_exact_charged_reservation() -> Result<(), HostError> {
    let fixture = crate::journal::request_lifecycle::tests::native_started_intake_fixture().await?;
    let completed_assignment = fixture
        .scope
        .assignment_identity(crate::journal::AssignmentKey::AssignedRequest {
            request_id: fixture.request_id,
        })
        .await?;
    assert_eq!(
        fixture
            .scope
            .record_job_completed(
                &completed_assignment,
                fixture.message_id,
                &fixture.source,
                Some("success"),
                Some(&fixture.runner),
                Some("opaque-started-job"),
            )
            .await?,
        Some(fixture.intent_id)
    );
    let launches = fixture.scope.due_completed_launches(0, 1).await?;
    assert_eq!(launches.len(), 1);
    let launch = &launches[0];
    assert_eq!(launch.intent.id, fixture.intent_id);
    assert!(launch.intent.worker_volume.is_none());

    let reservation = fixture
        .scope
        .native_completion_reservation(launch)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(reservation.reservation_id(), fixture.reservation_id);
    assert_eq!(reservation.intent_id(), fixture.intent_id);
    assert_eq!(
        reservation.runner_container_id(),
        fixture.channel.runner_container_id()
    );
    assert_eq!(
        reservation.dind_container_id(),
        fixture.channel.dind_container_id()
    );
    assert!(!reservation.remote_cleanup_proven());
    assert!(!reservation.storage_cleanup_proven());
    assert!(!reservation.local_cleanup_proven());
    Ok(())
}

#[tokio::test]
async fn native_capacity_retry_does_not_repeat_proven_cleanup_effects() -> Result<(), HostError> {
    let fixture = capacity_retry_fixture().await?;
    let (launch, now) = seed_completion(&fixture).await?;
    let first_claim = fixture
        .scope
        .claim_completion_cleanup_at(launch.intent.id, now, 90)
        .await?
        .ok_or(HostError::Journal)?;
    let first = run_native_cleanup(&fixture, &launch, first_claim).await?;
    assert!(matches!(
        first,
        Err(super::super::Failure {
            phase: "metadata capacity hold",
            ..
        })
    ));
    let after_first = fixture.server.effects();
    assert_eq!(after_first.observer_runs, 1);
    assert_eq!(after_first.capacity_runs, 1);
    assert_eq!(
        fixture
            .scope
            .reservation(&fixture._base.reservation_assignment)
            .await?
            .ok_or(HostError::Journal)?
            .state,
        crate::journal::ReservationState::Reserved
    );

    assert!(
        fixture
            .scope
            .retry_completion_cleanup_at(launch.intent.id, first_claim.generation, now, now + 1)
            .await?
    );
    let retry_claim = fixture
        .scope
        .claim_completion_cleanup_at(launch.intent.id, now + 1, 90)
        .await?
        .ok_or(HostError::Journal)?;
    let second = run_native_cleanup(&fixture, &launch, retry_claim).await?;
    assert!(second.is_ok());
    let after_retry = fixture.server.effects();
    assert_eq!(after_retry.observer_runs, after_first.observer_runs);
    assert_eq!(after_retry.container_deletes, after_first.container_deletes);
    assert_eq!(after_retry.volume_deletes, after_first.volume_deletes);
    assert_eq!(after_retry.network_deletes, after_first.network_deletes);
    assert_eq!(after_retry.capacity_runs, after_first.capacity_runs + 1);
    assert_eq!(
        fixture
            .scope
            .reservation(&fixture._base.reservation_assignment)
            .await?
            .ok_or(HostError::Journal)?
            .state,
        crate::journal::ReservationState::Released
    );
    Ok(())
}

async fn capacity_retry_fixture()
-> Result<crate::launch::bootstrap::CleanupRuntimeFixture, HostError> {
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
    crate::launch::bootstrap::cleanup_runtime_fixture_for_base(base).await
}

async fn seed_completion(
    fixture: &crate::launch::bootstrap::CleanupRuntimeFixture,
) -> Result<(crate::journal::CompletedLaunch, i64), HostError> {
    use std::time::{SystemTime, UNIX_EPOCH};

    let base = &fixture._base;
    let completed = fixture
        .scope
        .record_job_completed(
            &base.assignment,
            base.message_id,
            &base.source,
            Some("success"),
            Some(&base.runner),
            Some("opaque-started-job"),
        )
        .await?;
    assert_eq!(completed, Some(base.intent_id));
    fixture
        .scope
        .record_remote_cleanup(fixture.reservation_id)
        .await?;
    let now = i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| HostError::Journal)?
            .as_secs(),
    )
    .map_err(|_| HostError::Journal)?;
    let launch = fixture
        .scope
        .due_completed_launches(now, 1)
        .await?
        .into_iter()
        .next()
        .ok_or(HostError::Journal)?;
    Ok((launch, now))
}

async fn run_native_cleanup(
    fixture: &crate::launch::bootstrap::CleanupRuntimeFixture,
    launch: &crate::journal::CompletedLaunch,
    claim: crate::journal::CleanupClaim,
) -> Result<Result<(), super::super::Failure>, HostError> {
    use std::{os::unix::fs::PermissionsExt, sync::atomic::AtomicBool};

    use crate::{https::HttpsTransport, launch::completion, listen::Secret};

    let scratch = fixture._base._scratch.file();
    let state = std::fs::canonicalize(scratch.parent().ok_or(HostError::Path)?)
        .map_err(|_| HostError::Path)?;
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700))
        .map_err(|_| HostError::Path)?;
    let diagnostics = completion::diagnostics::DiagnosticsRoot::open(
        &state,
        fixture.scope.connection_id().as_str(),
    )?;
    let mut transport = HttpsTransport::new("https://github.com")?;
    let admin = Secret::new("native cleanup fixture");
    let journal = completion::journal_ref::CompletionJournal::scoped(fixture.scope.clone());
    let docker = fixture.server.docker();
    let engine = fixture
        .scope
        .engine_context()
        .await?
        .ok_or(HostError::Ownership)?;
    let info = docker.info().await.map_err(|_| HostError::Docker)?;
    let selected = crate::launch::SelectedEngineContext::capture(&engine, &info)?;
    let stopping = AtomicBool::new(false);
    let mut context = completion::cleanup::Context {
        journal: &journal,
        docker: &docker,
        transport: &mut transport,
        admin: &admin,
        diagnostics: &diagnostics,
        native_cleanup: Some(&fixture.runtime),
        selected_engine: Some(&selected),
    };
    let mut budget = super::super::EffectBudget::new_native(&stopping);
    Ok(super::cleanup(&mut context, launch, claim, &mut budget).await)
}
