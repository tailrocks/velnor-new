use crate::error::HostError;

pub(in crate::launch::completion::cleanup::native) async fn capacity_retry_fixture()
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

pub(in crate::launch::completion::cleanup::native) async fn seed_completion(
    fixture: &crate::launch::bootstrap::CleanupRuntimeFixture,
) -> Result<(crate::journal::CompletedLaunch, i64), HostError> {
    use std::time::{SystemTime, UNIX_EPOCH};

    let crate::launch::bootstrap::CleanupRuntimeFixture { _base: base, .. } = fixture;
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

pub(in crate::launch::completion::cleanup::native) async fn run_native_cleanup(
    fixture: &crate::launch::bootstrap::CleanupRuntimeFixture,
    launch: &crate::journal::CompletedLaunch,
    claim: crate::journal::CleanupClaim,
) -> Result<Result<(), crate::launch::completion::cleanup::Failure>, HostError> {
    let diagnostics = diagnostics_root(fixture)?;
    Box::pin(run_native_cleanup_with_diagnostics(
        fixture,
        launch,
        claim,
        &diagnostics,
    ))
    .await
}

pub(in crate::launch::completion::cleanup::native) fn diagnostics_root(
    fixture: &crate::launch::bootstrap::CleanupRuntimeFixture,
) -> Result<crate::launch::completion::diagnostics::DiagnosticsRoot, HostError> {
    use std::os::unix::fs::PermissionsExt;

    let crate::launch::bootstrap::CleanupRuntimeFixture { _base: base, .. } = fixture;
    let crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture {
        _scratch: scratch,
        ..
    } = base;
    let scratch = scratch.file();
    let state = std::fs::canonicalize(scratch.parent().ok_or(HostError::Path)?)
        .map_err(|_| HostError::Path)?;
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700))
        .map_err(|_| HostError::Path)?;
    crate::launch::completion::diagnostics::DiagnosticsRoot::open(
        &state,
        fixture.scope.connection_id().as_str(),
    )
}

pub(in crate::launch::completion::cleanup::native) async fn run_native_cleanup_with_diagnostics(
    fixture: &crate::launch::bootstrap::CleanupRuntimeFixture,
    launch: &crate::journal::CompletedLaunch,
    claim: crate::journal::CleanupClaim,
    diagnostics: &crate::launch::completion::diagnostics::DiagnosticsRoot,
) -> Result<Result<(), crate::launch::completion::cleanup::Failure>, HostError> {
    use std::sync::atomic::AtomicBool;

    use crate::{https::HttpsTransport, launch::completion, listen::Secret};

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
        diagnostics,
        native_cleanup: Some(&fixture.runtime),
        selected_engine: Some(&selected),
    };
    let mut budget = completion::cleanup::EffectBudget::new_native(&stopping);
    Ok(Box::pin(completion::cleanup::reconcile_claimed(
        &mut context,
        launch,
        claim,
        &mut budget,
    ))
    .await)
}
