use super::*;
use serde_json::Value;

#[tokio::test]
async fn cleanup_timeout_keeps_owned_container_retryable_after_reopen() -> Result<(), String> {
    let (scratch, journal, projection) = prepared().await?;
    let info = r#"{"ID":"probe-engine","DockerRootDir":"/var/lib/docker","NCPU":8,"MemTotal":17179869184}"#;
    let delay = Duration::from_millis(150);
    let mut responses = start_responses(&projection)?;
    responses.extend([
        http(200, "{}"),
        http(200, r#"{"StatusCode":0,"Error":null}"#),
        http(200, &inspect(&projection, "exited", 0)?),
        raw_http(200, log_frame(&record_bytes())?),
        delayed_http(200, info, delay),
        delayed_http(200, info, delay),
    ]);
    let docker = DockerStub::open(responses)?;
    let result = execute::run(
        &docker.docker,
        &journal,
        &projection,
        16 * 1024 * 1024 * 1024,
        &Deadline::test_with_timeouts(Duration::from_millis(800), Duration::from_millis(80)),
    )
    .await;
    assert!(matches!(result, Err(HostError::DockerTimeout)));
    let requests = docker.finish_observed().await?;
    assert!(requests.iter().any(|request| request.contains("/info")));
    let retained = journal
        .active_probe()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "timed-out probe row was discarded".to_owned())?;
    assert_eq!(retained.phase, crate::journal::ProbePhase::OutputObserved);
    assert_eq!(retained.container_id.as_deref(), Some(CONTAINER_ID));
    assert_persisted_projection_rebuilds(&retained)?;

    drop(journal);
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let exited = inspect(&projection, "exited", 0)?;
    let retry = vec![
        http(200, info),
        http(200, info),
        http(200, &exited),
        http(200, &exited),
        http(200, info),
        closed(),
        http(200, info),
        http(404, "{}"),
        http(404, "{}"),
        http(404, "{}"),
        http(404, "{}"),
        http(200, info),
    ];
    let recovery = DockerStub::open(retry)?;
    let result = Box::pin(lifecycle::collect(
        &recovery.docker,
        &journal,
        &UnavailableImageProvider,
    ))
    .await;
    let recovery_requests = recovery.finish_observed().await?;
    let sample = result.map_err(|error| format!("{error}; requests: {recovery_requests:?}"))?;
    assert!(sample.is_none());
    assert_eq!(recovery_requests.len(), 12);
    assert!(
        recovery_requests
            .iter()
            .any(|request| request.starts_with("DELETE "))
    );
    assert!(
        journal
            .active_probe()
            .await
            .map_err(|error| error.to_string())?
            .is_none()
    );
    Ok(())
}

fn assert_persisted_projection_rebuilds(row: &crate::journal::ProbeRow) -> Result<(), String> {
    let image = VerifiedProbeImage::from_journal(
        row.runtime_image_id.clone(),
        row.source_revision.clone(),
        row.image_binding_digest.clone(),
    )
    .map_err(|error| error.to_string())?;
    let replayed = ProbeProjection::build(
        row.operation_id.clone(),
        row.instance_id.clone(),
        row.engine_id.clone(),
        ROOT,
        image,
    )
    .map_err(|error| error.to_string())?;
    assert_eq!(row.projection_digest, replayed.projection_digest);
    Ok(())
}

#[tokio::test]
async fn held_journal_transition_timeout_sends_no_docker_request() -> Result<(), String> {
    let (_scratch, journal, projection) = prepared().await?;
    let _held = journal.write_guard().await;
    let docker = DockerStub::open(Vec::new())?;
    let result = execute::run(
        &docker.docker,
        &journal,
        &projection,
        16 * 1024 * 1024 * 1024,
        &Deadline::test_with_timeouts(Duration::from_millis(40), Duration::from_millis(20)),
    )
    .await;
    let requests = docker.finish_observed().await?;
    assert!(matches!(result, Err(HostError::DockerTimeout)));
    assert!(
        requests.is_empty(),
        "expired journal transition sent {requests:?}"
    );
    let retained = journal
        .active_probe()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "held transition removed its prepared row".to_owned())?;
    assert_eq!(retained.phase, crate::journal::ProbePhase::Prepared);
    Ok(())
}

#[tokio::test]
async fn failed_quarantine_commit_is_propagated_without_lifecycle_retry() -> Result<(), String> {
    let (_scratch, journal, projection) = prepared().await?;
    journal
        .transition_probe(
            &projection.operation_id,
            crate::journal::ProbePhase::Prepared,
            crate::journal::ProbePhase::CreateRequested,
        )
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_probe_container(&projection.operation_id, CONTAINER_ID)
        .await
        .map_err(|error| error.to_string())?;
    let row = journal
        .active_probe()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "probe row disappeared before recovery".to_owned())?;
    let info = r#"{"ID":"probe-engine","DockerRootDir":"/var/lib/docker","NCPU":8,"MemTotal":17179869184}"#;
    let mut malformed = serde_json::from_str::<Value>(&inspect(&projection, "created", 0)?)
        .map_err(|error| error.to_string())?;
    malformed["Config"]["WorkingDir"] = serde_json::json!("/tmp");
    let malformed = serde_json::to_string(&malformed).map_err(|error| error.to_string())?;
    let docker = DockerStub::open(vec![
        http(200, info),
        http(200, &malformed),
        http(200, &malformed),
    ])?;
    let held = journal.write_guard().await;
    let result = Box::pin(crate::launch::resource_probe::recover::active(
        &docker.docker,
        &journal,
        &serde_json::from_str::<bollard::models::SystemInfo>(info)
            .map_err(|error| error.to_string())?,
        &crate::launch::resource_probe::projection::DockerRoot::parse("/var/lib/docker")
            .map_err(|error| error.to_string())?,
        &Deadline::test_with_timeouts(Duration::from_millis(250), Duration::from_millis(30)),
        row,
    ))
    .await;
    drop(held);
    assert!(matches!(result, Err(HostError::DockerTimeout)));
    let requests = docker.finish_observed().await?;
    assert_eq!(requests.len(), 3);
    assert!(requests.iter().all(|request| !request.starts_with("POST ")));
    let retained = journal
        .active_probe()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "failed quarantine removed its active row".to_owned())?;
    assert_eq!(retained.phase, crate::journal::ProbePhase::ContainerCreated);
    assert_eq!(retained.container_id.as_deref(), Some(CONTAINER_ID));
    Ok(())
}
