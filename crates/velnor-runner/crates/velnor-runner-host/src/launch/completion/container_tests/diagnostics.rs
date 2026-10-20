//! Diagnostic capture and resource hold tests.

#[path = "child_crash.rs"]
mod child_crash;

use super::*;

#[tokio::test]
async fn claim_loss_after_capture_prevents_container_delete() -> Result<(), HostError> {
    let scratch = Scratch::open("capture-claim-loss")?;
    let journal = Journal::open(&scratch.path).await?;
    let launch = completed_launch(&journal).await?;
    let completion_journal = CompletionJournal::root(journal.clone());
    let claim = journal
        .claim_completion_cleanup(launch.intent.id, 90)
        .await?
        .ok_or(HostError::Journal)?;
    let (stub, capture_started, continue_capture) = DockerStub::open_paused(
        vec![
            response(200, &container_json(RUNNER_ID, "runner")),
            response(200, &container_json(RUNNER_ID, "runner")),
            response(200, &container_json(RUNNER_ID, "runner")),
        ],
        2,
    )?;
    let mut transport = HttpsTransport::new("https://github.com")?;
    let admin = Secret::new("test token");
    let diagnostics = diagnostics_root(&scratch)?;
    let cleanup_result = {
        let context = Context {
            journal: &completion_journal,
            docker: &stub.docker,
            transport: &mut transport,
            admin: &admin,
            diagnostics: &diagnostics,
            native_cleanup: None,
            selected_engine: None,
        };
        let cleanup = run_cleanup(&context, &launch, claim);
        tokio::pin!(cleanup);
        tokio::select! {
            _ = &mut cleanup => return Err(HostError::Journal),
            reached = capture_started => reached.map_err(|_| HostError::Docker)?,
        }
        if !journal
            .retry_completion_cleanup(launch.intent.id, claim.generation, 60)
            .await?
        {
            return Err(HostError::Journal);
        }
        continue_capture.send(()).map_err(|()| HostError::Docker)?;
        cleanup.await
    };
    assert_eq!(cleanup_result, Err(Failure::claim_lost()));
    let requests = stub.finish().await?;
    assert_eq!(requests.len(), 3);
    assert!(requests.iter().all(|request| request.starts_with("GET ")));
    let diagnostic_scope = scratch
        .directory
        .join(SCOPE_DIRECTORY)
        .join(scope_component("root"));
    if std::fs::read_dir(diagnostic_scope)
        .map_err(|_| HostError::Path)?
        .next()
        .is_some()
    {
        return Err(HostError::Ownership);
    }
    let rows = journal.rows().await?;
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].cleanup_proven);
    assert_eq!(journal.capacity_rows(1).await?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn pending_diagnostics_worker_blocks_cleanup_after_containers_are_absent()
-> Result<(), HostError> {
    let scratch = Scratch::open("pending-after-absence")?;
    let journal = Journal::open(&scratch.path).await?;
    let launch = completed_launch(&journal).await?;
    let completion_journal = CompletionJournal::root(journal.clone());
    let claim = journal
        .claim_completion_cleanup(launch.intent.id, 90)
        .await?
        .ok_or(HostError::Journal)?;
    let absent = r#"{"message":"missing"}"#;
    let stub = DockerStub::open(vec![
        response(404, absent),
        response(404, absent),
        response(404, absent),
        response(404, absent),
    ])?;
    let mut transport = HttpsTransport::new("https://github.com")?;
    let admin = Secret::new("test token");
    let diagnostics = diagnostics_root(&scratch)?;
    {
        let context = Context {
            journal: &completion_journal,
            docker: &stub.docker,
            transport: &mut transport,
            admin: &admin,
            diagnostics: &diagnostics,
            native_cleanup: None,
            selected_engine: None,
        };
        let stopping = AtomicBool::new(false);
        let mut budget = EffectBudget::new(&stopping);
        cleanup(&context, &launch, claim, &mut budget)
            .await
            .map_err(|failure| failure.error)?;
    }
    assert_eq!(stub.finish().await?.len(), 4);

    let pending = diagnostics.hold_pending_write_for_test(launch.intent.id, claim.generation)?;
    let blocked_stub = DockerStub::open(Vec::new())?;
    let result = {
        let mut context = Context {
            journal: &completion_journal,
            docker: &blocked_stub.docker,
            transport: &mut transport,
            admin: &admin,
            diagnostics: &diagnostics,
            native_cleanup: None,
            selected_engine: None,
        };
        let stopping = AtomicBool::new(false);
        let mut budget = EffectBudget::new(&stopping);
        super::super::super::reconcile_claimed(&mut context, &launch, claim, &mut budget).await
    };
    assert_eq!(result, Err(Failure::not_proven("diagnostic write pending")));
    assert_eq!(blocked_stub.finish().await?.len(), 0);
    let rows = journal.rows().await?;
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].cleanup_proven);
    assert_eq!(journal.capacity_rows(1).await?.len(), 1);
    pending.finish().await?;
    Ok(())
}

#[tokio::test]
async fn reopened_diagnostics_reject_partial_capture_before_container_delete()
-> Result<(), HostError> {
    let scratch = Scratch::open("reopened-partial-capture")?;
    let journal = Journal::open(&scratch.path).await?;
    let launch = completed_launch(&journal).await?;
    let completion_journal = CompletionJournal::root(journal.clone());
    let claim = journal
        .claim_completion_cleanup(launch.intent.id, 90)
        .await?
        .ok_or(HostError::Journal)?;
    let initial = diagnostics_root(&scratch)?;
    drop(initial);

    let scope = scratch
        .directory
        .join(SCOPE_DIRECTORY)
        .join(scope_component("root"));
    let filename = diagnostic_filename(launch.intent.id, claim.generation, "runner", RUNNER_ID);
    let partial = scope.join(filename);
    std::fs::write(&partial, b"{\"version\":1").map_err(|_| HostError::Path)?;
    std::fs::set_permissions(&partial, std::fs::Permissions::from_mode(0o600))
        .map_err(|_| HostError::Path)?;
    let before = std::fs::read(&partial).map_err(|_| HostError::Path)?;
    let diagnostics = diagnostics_root(&scratch)?;
    let stub = DockerStub::open(vec![
        response(200, &container_json(RUNNER_ID, "runner")),
        response(200, &container_json(RUNNER_ID, "runner")),
        response(200, &container_json(RUNNER_ID, "runner")),
    ])?;
    let mut transport = HttpsTransport::new("https://github.com")?;
    let admin = Secret::new("test token");
    let context = Context {
        journal: &completion_journal,
        docker: &stub.docker,
        transport: &mut transport,
        admin: &admin,
        diagnostics: &diagnostics,
        native_cleanup: None,
        selected_engine: None,
    };

    let result = run_cleanup(&context, &launch, claim).await;
    assert_eq!(
        result,
        Err(Failure::request("diagnostic write", HostError::Ownership))
    );
    let requests = stub.finish().await?;
    assert_eq!(requests.len(), 3);
    assert!(requests.iter().all(|request| request.starts_with("GET ")));
    assert_eq!(
        std::fs::read(&partial).map_err(|_| HostError::Path)?,
        before
    );
    let rows = journal.rows().await?;
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].cleanup_proven);
    assert_eq!(journal.capacity_rows(1).await?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn partial_file_from_crashed_child_does_not_authorize_cleanup() -> Result<(), HostError> {
    child_crash::run_partial_capture_child()?;
    let scratch = Scratch::open("child-crash-capture")?;
    let journal = Journal::open(&scratch.path).await?;
    let launch = completed_launch(&journal).await?;
    let completion_journal = CompletionJournal::root(journal.clone());
    let claim = journal
        .claim_completion_cleanup(launch.intent.id, 90)
        .await?
        .ok_or(HostError::Journal)?;
    let diagnostics = diagnostics_root(&scratch)?;
    let scope = scratch
        .directory
        .join(SCOPE_DIRECTORY)
        .join(scope_component("root"));
    let partial_path = scope.join(diagnostic_filename(
        launch.intent.id,
        claim.generation,
        "runner",
        RUNNER_ID,
    ));
    let engine_id = format!("engine-{}", uuid::Uuid::new_v4());
    let before =
        child_crash::spawn_child_with_partial_file(&partial_path, &scratch.directory, &engine_id)?;
    let _engine_guard = crate::daemon_lock::EngineLineageGuard::acquire_at_for_tests(
        &engine_id,
        &scratch.directory,
    )?;
    let stub = DockerStub::open(vec![
        response(200, &container_json(RUNNER_ID, "runner")),
        response(200, &container_json(RUNNER_ID, "runner")),
        response(200, &container_json(RUNNER_ID, "runner")),
    ])?;
    let mut transport = HttpsTransport::new("https://github.com")?;
    let admin = Secret::new("test token");
    let context = Context {
        journal: &completion_journal,
        docker: &stub.docker,
        transport: &mut transport,
        admin: &admin,
        diagnostics: &diagnostics,
        native_cleanup: None,
        selected_engine: None,
    };

    let result = run_cleanup(&context, &launch, claim).await;
    assert_eq!(
        result,
        Err(Failure::request("diagnostic write", HostError::Ownership))
    );
    let requests = stub.finish().await?;
    assert_eq!(requests.len(), 3);
    assert!(requests.iter().all(|request| request.starts_with("GET ")));
    assert_eq!(
        std::fs::read(&partial_path).map_err(|_| HostError::Path)?,
        before
    );
    let rows = journal.rows().await?;
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].cleanup_proven);
    assert_eq!(journal.capacity_rows(1).await?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn delayed_production_capture_returns_before_blocked_diagnostics_io() -> Result<(), HostError>
{
    if !std::env::var("DYLD_INSERT_LIBRARIES")
        .unwrap_or_default()
        .contains("fsync_delay")
    {
        return Ok(());
    }
    let scratch = Scratch::open("production-capture-timeout")?;
    let journal = Journal::open(&scratch.path).await?;
    let launch = completed_launch(&journal).await?;
    let completion_journal = CompletionJournal::root(journal.clone());
    let claim = journal
        .claim_completion_cleanup(launch.intent.id, 90)
        .await?
        .ok_or(HostError::Journal)?;
    let parent = scratch.directory.join(SCOPE_DIRECTORY);
    let scope = parent.join(scope_component("root"));
    for directory in [&parent, &scope] {
        std::fs::create_dir_all(directory).map_err(|_| HostError::Path)?;
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| HostError::Path)?;
    }
    let diagnostics = diagnostics_root(&scratch)?;
    let stub = DockerStub::open(vec![response(200, &container_json(RUNNER_ID, "runner"))])?;
    let mut transport = HttpsTransport::new("https://github.com")?;
    let admin = Secret::new("test token");
    let context = Context {
        journal: &completion_journal,
        docker: &stub.docker,
        transport: &mut transport,
        admin: &admin,
        diagnostics: &diagnostics,
        native_cleanup: None,
        selected_engine: None,
    };
    let stopping = AtomicBool::new(false);
    let mut budget = EffectBudget::new(&stopping);
    let started = std::time::Instant::now();
    let result = crate::launch::completion::diagnostics::capture(
        &context,
        crate::launch::completion::diagnostics::CaptureRequest {
            root: &diagnostics,
            launch: &launch,
            claim,
            budget: &mut budget,
            role: "runner",
            expected_name: &format!("{WORKER}-runner"),
            container_id: RUNNER_ID,
            volume: WORKER,
        },
    )
    .await;
    let elapsed = started.elapsed();
    assert_eq!(
        result,
        Err(Failure::request(
            "diagnostic write",
            HostError::DockerTimeout
        ))
    );
    assert!(elapsed >= Duration::from_secs(14));
    assert!(elapsed < Duration::from_millis(15_900));
    assert!(diagnostics.pending_for(launch.intent.id)?);
    tokio::time::timeout(Duration::from_secs(3), async {
        while diagnostics.pending_for(launch.intent.id).unwrap_or(true) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .map_err(|_| HostError::DockerTimeout)?;
    assert_eq!(stub.finish().await?.len(), 1);
    let rows = journal.rows().await?;
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].cleanup_proven);
    assert_eq!(journal.capacity_rows(1).await?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn home_volume_delete_failure_keeps_capacity_until_cleanup_proof() -> Result<(), HostError> {
    let scratch = Scratch::open("home-volume-hold")?;
    let journal = Journal::open(&scratch.path).await?;
    let launch = completed_launch(&journal).await?;
    let completion_journal = CompletionJournal::root(journal.clone());
    let claim = journal
        .claim_completion_cleanup(launch.intent.id, 90)
        .await?
        .ok_or(HostError::Journal)?;
    let stub = DockerStub::open(vec![
        response(404, r#"{"message":"missing"}"#),
        response(200, &volume_json("wcompletion-home", WORKER, "home")),
        response(500, r#"{"message":"busy"}"#),
    ])?;
    let mut transport = HttpsTransport::new("https://github.com")?;
    let admin = Secret::new("test token");
    let diagnostics = diagnostics_root(&scratch)?;
    let context = Context {
        journal: &completion_journal,
        docker: &stub.docker,
        transport: &mut transport,
        admin: &admin,
        diagnostics: &diagnostics,
        native_cleanup: None,
        selected_engine: None,
    };
    let stopping = AtomicBool::new(false);
    let mut budget = EffectBudget::new(&stopping);

    let result = volume::cleanup(&context, &launch, claim, &mut budget).await;
    assert_eq!(
        result,
        Err(Failure::request("volume delete", HostError::Docker))
    );
    let requests = stub.finish().await?;
    assert_eq!(requests.len(), 3);
    assert!(requests[1].contains("wcompletion-home"));
    assert!(requests[2].starts_with("DELETE "));
    let rows = journal.rows().await?;
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].cleanup_proven);
    assert_eq!(journal.capacity_rows(1).await?.len(), 1);
    Ok(())
}

#[path = "diagnostics/metadata_volume.rs"]
mod metadata_volume;

#[path = "diagnostics/post_delete.rs"]
mod post_delete;
