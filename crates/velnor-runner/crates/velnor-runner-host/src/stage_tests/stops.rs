use super::*;

#[tokio::test]
async fn expired_admission_stops_before_the_first_docker_effect() -> Result<(), HostError> {
    let engine = Fake::new();
    let input = TestGuestImage::new()?;
    let result = drive(
        &engine,
        "worker_a",
        test_resource_budget()?,
        b"opaque-jit",
        PairStop::Jit,
        &input.connection,
        &input.images,
        &input.selected_engine,
        &input.file,
        StageAdmission::test_stale_fixture_for(&input.selected_engine),
        &Forget,
    )
    .await;
    assert_eq!(result, Err(HostError::Ownership));
    assert!(engine.events().is_empty());
    Ok(())
}

#[tokio::test]
async fn dind_created_does_not_start() -> Result<(), HostError> {
    let engine = Fake::new();
    let partial = drive_test(
        &engine,
        "worker_a",
        test_resource_budget()?,
        b"jit",
        PairStop::DindCreated,
        &Forget,
    )
    .await?;
    assert_eq!(partial.dind_id.as_deref(), Some(FIRST_CONTAINER_ID));
    assert_eq!(partial.runner_id, None);
    assert_eq!(engine.events(), ["volumes", "create"]);
    Ok(())
}

#[tokio::test]
async fn remove_recorded_deletes_only_the_owned_id() -> Result<(), HostError> {
    let engine = Fake::new();
    engine
        .names
        .lock()
        .map_err(|_| HostError::Docker)?
        .insert("runner".to_owned(), "aaaaaaaaaaaa".to_owned());
    let decision = decide(&engine, "aaaaaaaaaaaa", "runner").await?;
    assert_eq!(decision, crate::docker_spec::DeleteDecision::Delete);
    assert_eq!(engine.events(), ["remove"]);
    Ok(())
}

#[tokio::test]
async fn remove_recorded_keeps_a_foreign_id() -> Result<(), HostError> {
    let engine = Fake::new();
    engine
        .names
        .lock()
        .map_err(|_| HostError::Docker)?
        .insert("runner".to_owned(), "bbbbbbbbbbbb".to_owned());
    let decision = decide(&engine, "aaaaaaaaaaaa", "runner").await?;
    assert_eq!(decision, crate::docker_spec::DeleteDecision::KeepForeign);
    assert_eq!(engine.events(), Vec::<&str>::new());
    Ok(())
}

#[tokio::test]
async fn volumes_stop_creates_no_container() -> Result<(), HostError> {
    let engine = Fake::new();
    let partial = drive_test(
        &engine,
        "worker_a",
        test_resource_budget()?,
        b"jit",
        PairStop::Volumes,
        &Forget,
    )
    .await?;
    assert_eq!(partial.dind_id, None);
    assert_eq!(partial.runner_id, None);
    assert_eq!(engine.events(), ["volumes"]);
    Ok(())
}

#[tokio::test]
async fn dind_started_does_not_create_the_runner() -> Result<(), HostError> {
    let engine = Fake::new();
    let partial = drive_test(
        &engine,
        "worker_a",
        test_resource_budget()?,
        b"jit",
        PairStop::DindStarted,
        &Forget,
    )
    .await?;
    assert_eq!(partial.dind_id.as_deref(), Some(FIRST_CONTAINER_ID));
    assert_eq!(partial.runner_id, None);
    assert_eq!(engine.events(), ["volumes", "create", "start"]);
    Ok(())
}

#[tokio::test]
async fn runner_created_does_not_start() -> Result<(), HostError> {
    let engine = Fake::new();
    let partial = drive_test(
        &engine,
        "worker_a",
        test_resource_budget()?,
        b"jit",
        PairStop::RunnerCreated,
        &Forget,
    )
    .await?;
    assert!(partial.dind_id.is_some() && partial.runner_id.is_some());
    assert_eq!(engine.events(), ["volumes", "create", "start", "create"]);
    Ok(())
}

#[tokio::test]
async fn runner_started_does_not_write_jit() -> Result<(), HostError> {
    let engine = Fake::new();
    let partial = drive_test(
        &engine,
        "worker_a",
        test_resource_budget()?,
        b"jit",
        PairStop::RunnerStarted,
        &Forget,
    )
    .await?;
    assert!(partial.runner_id.is_some());
    assert_eq!(
        engine.events(),
        ["volumes", "create", "start", "create", "start"]
    );
    Ok(())
}

#[tokio::test]
async fn second_create_failure_removes_only_the_owned_dind() -> Result<(), HostError> {
    let engine = Fake::new();
    *engine.fail_at.lock().map_err(|_| HostError::Docker)? = Some(("create", 2));
    engine
        .names
        .lock()
        .map_err(|_| HostError::Docker)?
        .insert("foreign".to_owned(), "bbbbbbbbbbbb".to_owned());
    let Err(error) = drive_test(
        &engine,
        "worker_a",
        test_resource_budget()?,
        b"jit",
        PairStop::Jit,
        &Forget,
    )
    .await
    else {
        return Err(HostError::Docker);
    };
    assert_eq!(error, HostError::Docker);
    assert_eq!(engine.removed(), [FIRST_CONTAINER_ID.to_owned()]);
    assert_eq!(engine.events(), ["volumes", "create", "start", "remove"]);
    let names = engine.names.lock().map_err(|_| HostError::Docker)?;
    assert_eq!(
        names.get("foreign").map(String::as_str),
        Some("bbbbbbbbbbbb")
    );
    Ok(())
}

#[tokio::test]
async fn missing_name_is_not_deleted() -> Result<(), HostError> {
    let engine = Fake::new();
    let decision = decide(&engine, "aaaaaaaaaaaa", "absent").await?;
    assert_eq!(decision, crate::docker_spec::DeleteDecision::NotDeleted);
    assert_eq!(engine.events(), Vec::<&str>::new());
    Ok(())
}
