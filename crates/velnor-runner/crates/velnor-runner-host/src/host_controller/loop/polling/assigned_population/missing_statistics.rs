use std::collections::BTreeMap;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use crate::{
    error::HostError,
    host_controller::loop_driver::{polling::PositivePollContext, state::PreparedConnection},
    journal::ConnectionJournal,
    launch::{SelectedEngineContext, VerifiedWorkerImages},
};

#[test]
fn missing_statistics_poll_claims_orphan_and_keeps_capacity() -> Result<(), String> {
    crate::launch::test_harness_stack::run_on_test_harness_stack(
        "missing-statistics-retirement-cached-census",
        || Box::pin(missing_statistics_poll_case(Some(1))),
    )
}

#[test]
fn missing_statistics_poll_reconciles_before_initial_none_guard() -> Result<(), String> {
    crate::launch::test_harness_stack::run_on_test_harness_stack(
        "missing-statistics-retirement-empty-census",
        || Box::pin(missing_statistics_poll_case(None)),
    )
}

async fn missing_statistics_poll_case(initial_assigned_total: Option<i64>) -> Result<(), String> {
    let fixture = Box::pin(retirement_fixture()).await.map_err(show_error)?;
    recover_startup(&fixture._base).await?;
    assert_one_recoverable_candidate(&fixture.scope, fixture.reservation_id).await?;

    let binding =
        crate::journal::request_lifecycle::tests::helpers::binding("native-started-intake", 101)
            .map_err(show_error)?;
    let config =
        crate::config::HostConfig::test_with_job_authority_policy(&binding).map_err(show_error)?;
    let (images, selected) = selected_engine(&fixture.scope, fixture.reservation_id)
        .await
        .map_err(show_error)?;
    let secret = Arc::new(crate::launch::ReleaseReadSecret::test_token());
    let provider = crate::launch::IntakeCompositionProvider::new(&selected, images.source_sha());
    let state = fixture
        ._base
        .database_path
        .parent()
        .ok_or_else(|| "The retirement fixture has no state directory.".to_owned())?;
    let docker = fixture.server.docker();
    let context = PositivePollContext {
        config: &config,
        journal: &fixture.journal,
        docker: &docker,
        selected_engine: &selected,
        worker_images: &images,
        state,
        release_read_secret: &secret,
        admission_ready: true,
        start_permit_provider: Some(&provider),
    };
    let mut prepared = prepared_connection(&fixture.scope, &binding, &images, state)
        .await
        .map_err(show_error)?;
    prepared.assigned_total = initial_assigned_total;
    let admissions = AtomicUsize::new(0);
    let outcome = poll_without_statistics(&fixture.scope, &mut prepared, &admissions).await?;
    assert_eq!(outcome.assigned_total, None);
    assert_eq!(admissions.load(Ordering::Relaxed), 0);
    prepared.record_assigned_total(outcome.assigned_total);
    assert_eq!(prepared.assigned_total, initial_assigned_total);

    super::reconcile(&mut prepared, &context)
        .await
        .map_err(show_error)?;
    assert_retirement_claimed_but_charged(&fixture).await?;
    assert_eq!(prepared.assigned_total, initial_assigned_total);
    assert_eq!(
        fixture
            .journal
            .occupied_reservation_count()
            .await
            .map_err(show_error)?,
        1
    );
    assert_eq!(
        fixture
            .scope
            .physical_runner_reservations()
            .await
            .map_err(show_error)?
            .len(),
        1
    );
    assert_eq!(admissions.load(Ordering::Relaxed), 0);
    Ok(())
}

async fn poll_without_statistics(
    scope: &ConnectionJournal,
    prepared: &mut PreparedConnection,
    admissions: &AtomicUsize,
) -> Result<crate::launch::PositivePollOutcome, String> {
    let allocation = scope
        .session_header_allocation(prepared.session_intent)
        .await
        .map_err(show_error)?;
    let mut link = crate::listen::inert_link_for_offline_test();
    let mut response = SessionResponse;
    let mut session = velnor_runner_github::create_session(
        &mut response,
        prepared.set_id,
        "owner-101",
        "test-token",
    )
    .map_err(show_error)?;
    crate::launch::test_positive_poll_once_with_response(
        crate::launch::PositivePollContext {
            link: &mut link,
            set_id: prepared.set_id,
            session: &mut session,
            scoped: scope,
            session_intent_id: prepared.session_intent,
            allocation: &allocation,
            cursor: 0,
        },
        crate::launch::TestPollResponse {
            polled: velnor_runner_github::Poll::Batch(velnor_runner_github::ParsedBatch {
                message_id: 92_001,
                raw_body: String::new(),
                statistics: None,
                jobs: Vec::new(),
            }),
            path: "https://queue.example.invalid/messages".to_owned(),
            queue: Some("queue-token".to_owned()),
        },
        async |_link, _session, _path, _queue, _polled| Ok(()),
        async |_link, _poll, _origin, _path, _queue, _admin| {
            admissions.fetch_add(1, Ordering::Relaxed);
            Err(crate::scale_set::EnsureError::AdmissionClosed)
        },
    )
    .await
    .map_err(show_error)
}

async fn prepared_connection(
    scope: &ConnectionJournal,
    binding: &crate::config::RepositoryConnection,
    images: &VerifiedWorkerImages,
    state: &std::path::Path,
) -> Result<PreparedConnection, HostError> {
    let pending = scope
        .prepare_session_create("0123456789abcdef0123456789abcdef")
        .await?;
    let claim = scope
        .claim_session_create(pending.local_nonce())
        .await?
        .ok_or(HostError::Journal)?;
    let bound = scope
        .record_created_session(&claim, "missing-statistics-session", 1)
        .await?;
    let set_id = scope.scale_set().await?.ok_or(HostError::Journal)?.id;
    let guest_image = crate::worker::guest_image::write_guest_image(state, binding, images)
        .map_err(|_| HostError::Path)?;
    Ok(PreparedConnection {
        binding: binding.clone(),
        scope: scope.clone(),
        set_id,
        session_intent: bound.intent_id(),
        assigned_total: Some(1),
        poll_cursor: 0,
        poll_retry_at: None,
        guest_image,
        completion: None,
        credentials: None,
        runner_ready: crate::launch::scoped_runtime::RunnerReadyRegistry::new(),
        ready_intents: BTreeMap::default(),
        authorizing: BTreeMap::default(),
        pending_bootstrap: BTreeMap::default(),
        unkeyed_bootstrap: Vec::new(),
        authorized: BTreeMap::default(),
        pending_authorization: BTreeMap::default(),
        unkeyed_authorization: Vec::new(),
        pending_acquired_recovery: BTreeMap::default(),
        unkeyed_acquired_recovery: Vec::new(),
    })
}

async fn selected_engine(
    scope: &ConnectionJournal,
    reservation_id: i64,
) -> Result<(VerifiedWorkerImages, SelectedEngineContext), HostError> {
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
    let selected = SelectedEngineContext::bind(&engine, &images)?;
    Ok((images, selected))
}

async fn assert_one_recoverable_candidate(
    scope: &ConnectionJournal,
    reservation_id: i64,
) -> Result<(), String> {
    let candidates = scope
        .recoverable_post_jit_retirements()
        .await
        .map_err(show_error)?;
    if candidates.len() != 1 || candidates[0].reservation_id() != reservation_id {
        return Err(
            "Startup did not provide the expected orphaned physical attempt for retirement."
                .to_owned(),
        );
    }
    Ok(())
}

async fn assert_retirement_claimed_but_charged(
    fixture: &crate::launch::bootstrap::CleanupRuntimeFixture,
) -> Result<(), String> {
    let claim = fixture
        .scope
        .persisted_post_jit_retirement_claim_for_test(fixture.reservation_id)
        .await
        .map_err(show_error)?;
    if claim.claim_generation() != 1
        || fixture
            .scope
            .storage_cleanup_identity(fixture.reservation_id)
            .await
            .map_err(show_error)?
            .is_none()
    {
        return Err(
            "The poll without job statistics did not claim the orphaned physical attempt and retain its capacity charge."
                .to_owned(),
        );
    }
    if !fixture
        .scope
        .recoverable_post_jit_retirements()
        .await
        .map_err(show_error)?
        .is_empty()
    {
        return Err(
            "Recovery still returned the active claim as a new retirement candidate.".to_owned(),
        );
    }
    Ok(())
}

async fn retirement_fixture() -> Result<crate::launch::bootstrap::CleanupRuntimeFixture, HostError>
{
    crate::launch::bootstrap::cleanup_runtime_fixture().await
}

async fn recover_startup(
    fixture: &crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture,
) -> Result<(), String> {
    let canonical =
        crate::daemon_lock::canonical_journal_path(&fixture.database_path).map_err(show_error)?;
    let state_dir = fixture
        .database_path
        .parent()
        .ok_or_else(|| "The retirement fixture has no state directory.".to_owned())?;
    let lock_path = state_dir.join("daemon.lock");
    let daemon = crate::daemon_lock::DaemonLock::try_acquire(&lock_path).map_err(show_error)?;
    let engine_id = fixture.journal.engine_id().await.map_err(show_error)?;
    let guard = crate::daemon_lock::EngineLineageGuard::acquire_at_for_tests(&engine_id, state_dir)
        .map_err(show_error)?;
    let instance_id = fixture.journal.instance_id().await.map_err(show_error)?;
    let revision = fixture.journal.revision().await.map_err(show_error)?;
    guard
        .verify_lineage(&canonical, &instance_id, revision)
        .map_err(show_error)?;
    fixture
        .journal
        .attach_lineage_guard(guard.clone())
        .map_err(show_error)?;
    fixture
        .journal
        .bootstrap_header_authority()
        .await
        .map_err(show_error)?;
    let instance_id = fixture.journal.instance_id().await.map_err(show_error)?;
    let revision = fixture.journal.revision().await.map_err(show_error)?;
    let authority = crate::launch::SessionRefreshStartupAuthority::test_only(
        &daemon,
        &lock_path,
        &canonical,
        &instance_id,
        &engine_id,
        revision,
        &guard,
    )
    .map_err(show_error)?;
    fixture
        .journal
        .recover_interrupted_host_work(authority)
        .await
        .map_err(show_error)
}

fn show_error(error: impl std::fmt::Debug) -> String {
    format!("{error:?}")
}

struct SessionResponse;

impl velnor_runner_github::Transport for SessionResponse {
    fn exchange(
        &mut self,
        request: &velnor_runner_github::SessionRequest,
    ) -> Result<velnor_runner_github::Exchange, velnor_runner_github::TransportFail> {
        if request.method != velnor_runner_github::Method::Post {
            return Err(velnor_runner_github::TransportFail::Reset);
        }
        Ok(velnor_runner_github::Exchange {
            status: 200,
            body: br#"{"sessionId":"missing-statistics-session","messageQueueUrl":"https://queue.example.invalid/messages","messageQueueAccessToken":"queue-token"}"#.to_vec(),
        })
    }
}
