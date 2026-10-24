use super::*;

#[test]
fn runner_log_exec_attempts_stop_at_the_persisted_limit() -> Result<(), String> {
    run_serialized_diagnostic_test(
        "runner_log_exec_attempts_stop_at_the_persisted_limit",
        || {
            Box::pin(async {
                runner_log_exec_attempt_limit_case()
                    .await
                    .map_err(|error| error.to_string())
            })
        },
    )
}

async fn runner_log_exec_attempt_limit_case() -> Result<(), HostError> {
    let fixture = Box::pin(capacity_retry_fixture()).await?;
    fixture
        .server
        .expose_owned_containers(fixture.scope.connection_id().as_str())
        .await?;
    fixture.server.fail_runner_log_execs(3).await;
    let (launch, _) = seed_completion(&fixture).await?;
    let claim = fixture
        .scope
        .claim_completion_cleanup(launch.intent.id, 90)
        .await?
        .ok_or(HostError::Journal)?;
    let root = diagnostics_root(&fixture)?;

    let result = Box::pin(run_native_cleanup_with_diagnostics(
        &fixture, &launch, claim, &root,
    ))
    .await?;
    assert_eq!(
        result,
        Err(completion::cleanup::Failure::not_proven(
            "runner log output unavailable"
        ))
    );
    assert_eq!(fixture.server.runner_log_exec_count().await, 3);
    assert_eq!(fixture.server.effects().container_deletes, 0);
    assert_eq!(fixture.server.effects().volume_deletes, 0);
    let state: serde_json::Value = serde_json::from_slice(
        &std::fs::read(runner_log_state_path(&fixture, launch.intent.id)?)
            .map_err(|_| HostError::Path)?,
    )
    .map_err(|_| HostError::Frame)?;
    if state["phase"] != "terminal" || state["attempt"] != 3 {
        return Err(HostError::Ownership);
    }
    Ok(())
}

#[test]
fn projection_dispatches_stop_after_three_persisted_attempts() -> Result<(), String> {
    run_serialized_diagnostic_test(
        "projection_dispatches_stop_after_three_persisted_attempts",
        || {
            Box::pin(async {
                projection_dispatch_attempt_limit_case()
                    .await
                    .map_err(|error| error.to_string())
            })
        },
    )
}

async fn projection_dispatch_attempt_limit_case() -> Result<(), HostError> {
    let fixture = Box::pin(capacity_retry_fixture()).await?;
    fixture
        .server
        .expose_owned_containers(fixture.scope.connection_id().as_str())
        .await?;
    let (launch, _) = seed_completion(&fixture).await?;
    let claim = fixture
        .scope
        .claim_completion_cleanup(launch.intent.id, 90)
        .await?
        .ok_or(HostError::Journal)?;
    let before = crate::launch::completion::diagnostics::runner_log_submission_count();
    crate::launch::completion::diagnostics::fail_next_runner_log_writes(3);

    for attempt in 1..=3 {
        // Reopen the diagnostics root so each cleanup call must recover the
        // persisted attempt state rather than reuse an in-memory handle.
        let root = diagnostics_root(&fixture)?;
        let result = Box::pin(run_native_cleanup_with_diagnostics(
            &fixture, &launch, claim, &root,
        ))
        .await?;
        drop(root);
        assert_eq!(
            result,
            Err(completion::cleanup::Failure::request(
                "runner log write",
                HostError::Path,
            ))
        );
        let state: serde_json::Value = serde_json::from_slice(
            &std::fs::read(runner_log_state_path(&fixture, launch.intent.id)?)
                .map_err(|_| HostError::Path)?,
        )
        .map_err(|_| HostError::Frame)?;
        if state["phase"] != "projection_pending" || state["attempt"] != attempt {
            return Err(HostError::Ownership);
        }
    }
    if crate::launch::completion::diagnostics::runner_log_submission_count() != before + 3 {
        return Err(HostError::Ownership);
    }

    let root = diagnostics_root(&fixture)?;
    let result = Box::pin(run_native_cleanup_with_diagnostics(
        &fixture, &launch, claim, &root,
    ))
    .await?;
    drop(root);
    assert_eq!(
        result,
        Err(completion::cleanup::Failure::request(
            "runner log retry",
            HostError::Ownership,
        ))
    );
    if crate::launch::completion::diagnostics::runner_log_submission_count() != before + 3 {
        return Err(HostError::Ownership);
    }
    assert_eq!(fixture.server.effects().container_deletes, 0);
    assert_eq!(fixture.server.effects().volume_deletes, 0);
    Ok(())
}
