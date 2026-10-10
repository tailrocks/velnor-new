use super::*;

use bollard::models::SystemInfo;

use crate::launch::resource_probe::provider::{ImageProvider, VerifiedCandidate};

struct FixtureProvider;

impl ImageProvider for FixtureProvider {
    async fn verified_candidate(
        &self,
        _docker: &bollard::Docker,
        _engine_id: &str,
        _info: &SystemInfo,
    ) -> Result<Option<VerifiedCandidate>, HostError> {
        std::future::ready(()).await;
        Ok(Some(VerifiedCandidate {
            runtime_id: RUNTIME_ID.to_owned(),
            platform: "linux/amd64".to_owned(),
            source_revision: REVISION.to_owned(),
            binding_fingerprint: BINDING.to_owned(),
            config: expected_image_config(REVISION),
        }))
    }
}

#[tokio::test]
async fn engine_change_before_create_blocks_mutation_and_keeps_row_recoverable()
-> Result<(), String> {
    let scratch = Scratch::new("probe-engine-before-create").map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_engine(ENGINE_ID)
        .await
        .map_err(|error| error.to_string())?;
    let info_a = r#"{"ID":"probe-engine","DockerRootDir":"/var/lib/docker","NCPU":8,"MemTotal":17179869184}"#;
    let info_b = r#"{"ID":"other-engine","DockerRootDir":"/var/lib/docker","NCPU":8,"MemTotal":17179869184}"#;
    let docker = DockerStub::open(vec![
        http(200, info_a),
        http(200, info_b),
        http(200, info_b),
    ])?;
    let result = Box::pin(lifecycle::collect(
        &docker.docker,
        &journal,
        &FixtureProvider,
    ))
    .await;
    let requests = docker.finish_observed().await?;
    assert!(matches!(result, Err(HostError::Identity)));
    assert_eq!(requests.len(), 3);
    assert!(requests.iter().all(|request| request.starts_with("GET ")));
    assert!(
        journal
            .active_probe()
            .await
            .map_err(|error| error.to_string())?
            .is_some_and(|row| row.phase == crate::journal::ProbePhase::CreateRequested)
    );
    Ok(())
}

#[tokio::test]
async fn root_change_before_create_blocks_mutation_then_recovers_original_binding()
-> Result<(), String> {
    let scratch = Scratch::new("probe-root-before-create").map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_engine(ENGINE_ID)
        .await
        .map_err(|error| error.to_string())?;
    let original = r#"{"ID":"probe-engine","DockerRootDir":"/var/lib/docker","NCPU":8,"MemTotal":17179869184}"#;
    let replacement = r#"{"ID":"probe-engine","DockerRootDir":"/var/lib/docker-next","NCPU":8,"MemTotal":17179869184}"#;
    let changed = DockerStub::open(vec![
        http(200, original),
        http(200, replacement),
        http(200, replacement),
    ])?;
    let result = Box::pin(lifecycle::collect(
        &changed.docker,
        &journal,
        &FixtureProvider,
    ))
    .await;
    let changed_requests = changed.finish_observed().await?;
    assert!(matches!(result, Err(HostError::Identity)));
    assert!(
        changed_requests
            .iter()
            .all(|request| request.starts_with("GET "))
    );
    assert!(
        journal
            .active_probe()
            .await
            .map_err(|error| error.to_string())?
            .is_some_and(|row| row.phase == crate::journal::ProbePhase::CreateRequested)
    );

    let recovery = DockerStub::open(vec![
        http(200, original),
        http(200, original),
        http(404, "{}"),
    ])?;
    let result = Box::pin(lifecycle::collect(
        &recovery.docker,
        &journal,
        &UnavailableImageProvider,
    ))
    .await;
    let recovery_requests = recovery.finish_observed().await?;
    assert!(result.map_err(|error| error.to_string())?.is_none());
    assert_eq!(recovery_requests.len(), 3);
    assert!(
        journal
            .active_probe()
            .await
            .map_err(|error| error.to_string())?
            .is_none()
    );
    Ok(())
}

#[tokio::test]
async fn engine_change_after_create_blocks_start_then_cleans_on_bound_engine() -> Result<(), String>
{
    let (_scratch, journal, projection) = prepared().await?;
    let info_a = r#"{"ID":"probe-engine","DockerRootDir":"/var/lib/docker"}"#;
    let info_a_full = r#"{"ID":"probe-engine","DockerRootDir":"/var/lib/docker","NCPU":8,"MemTotal":17179869184}"#;
    let info_b = r#"{"ID":"other-engine","DockerRootDir":"/var/lib/docker"}"#;
    let changed = DockerStub::open(vec![
        http(200, info_a),
        http(200, &format!(r#"{{"Id":"{CONTAINER_ID}","Warnings":[]}}"#)),
        http(200, &inspect(&projection, "created", 0)?),
        http(200, &inspect(&projection, "created", 0)?),
        http(200, info_b),
        http(200, info_b),
    ])?;
    let result = execute::run(
        &changed.docker,
        &journal,
        &projection,
        16 * 1024 * 1024 * 1024,
        &Deadline::new(),
    )
    .await;
    let changed_requests = changed.finish_observed().await?;
    assert!(matches!(result, Err(HostError::Identity)));
    assert!(
        !changed_requests
            .iter()
            .any(|request| request.ends_with("/start HTTP/1.1"))
    );
    assert!(
        !changed_requests
            .iter()
            .any(|request| request.starts_with("DELETE "))
    );
    assert_eq!(
        journal
            .active_probe()
            .await
            .map_err(|error| error.to_string())?
            .map(|row| row.phase),
        Some(crate::journal::ProbePhase::StartRequested)
    );

    let retry = DockerStub::open(vec![
        http(200, info_a_full),
        http(200, info_a),
        http(200, &inspect(&projection, "created", 0)?),
        http(200, &inspect(&projection, "created", 0)?),
        http(200, info_a),
        closed(),
        http(200, info_a),
        http(404, "{}"),
        http(404, "{}"),
        http(404, "{}"),
        http(404, "{}"),
        http(200, info_a),
    ])?;
    let result = Box::pin(lifecycle::collect(
        &retry.docker,
        &journal,
        &UnavailableImageProvider,
    ))
    .await;
    let retry_requests = retry.finish_observed().await?;
    let sample = result.map_err(|error| format!("{error}; requests: {retry_requests:?}"))?;
    assert!(sample.is_none());
    assert!(
        retry_requests
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

#[tokio::test]
async fn root_change_after_create_blocks_start_then_cleans_on_original_root() -> Result<(), String>
{
    let (_scratch, journal, projection) = prepared().await?;
    let original = r#"{"ID":"probe-engine","DockerRootDir":"/var/lib/docker"}"#;
    let original_full = r#"{"ID":"probe-engine","DockerRootDir":"/var/lib/docker","NCPU":8,"MemTotal":17179869184}"#;
    let replacement = r#"{"ID":"probe-engine","DockerRootDir":"/var/lib/docker-next"}"#;
    let changed = DockerStub::open(vec![
        http(200, original),
        http(200, &format!(r#"{{"Id":"{CONTAINER_ID}","Warnings":[]}}"#)),
        http(200, &inspect(&projection, "created", 0)?),
        http(200, &inspect(&projection, "created", 0)?),
        http(200, replacement),
        http(200, replacement),
    ])?;
    let result = execute::run(
        &changed.docker,
        &journal,
        &projection,
        16 * 1024 * 1024 * 1024,
        &Deadline::new(),
    )
    .await;
    let changed_requests = changed.finish_observed().await?;
    assert!(matches!(result, Err(HostError::Identity)));
    assert!(
        !changed_requests
            .iter()
            .any(|request| request.ends_with("/start HTTP/1.1"))
    );
    assert!(
        !changed_requests
            .iter()
            .any(|request| request.starts_with("DELETE "))
    );
    assert_eq!(
        journal
            .active_probe()
            .await
            .map_err(|error| error.to_string())?
            .map(|row| row.phase),
        Some(crate::journal::ProbePhase::StartRequested)
    );

    let recovery = DockerStub::open(vec![
        http(200, original_full),
        http(200, original),
        http(200, &inspect(&projection, "created", 0)?),
        http(200, &inspect(&projection, "created", 0)?),
        http(200, original),
        http(204, ""),
        http(200, original),
        http(404, "{}"),
        http(404, "{}"),
        http(404, "{}"),
        http(404, "{}"),
        http(200, original),
    ])?;
    let result = Box::pin(lifecycle::collect(
        &recovery.docker,
        &journal,
        &UnavailableImageProvider,
    ))
    .await;
    let recovery_requests = recovery.finish_observed().await?;
    assert!(result.map_err(|error| error.to_string())?.is_none());
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
