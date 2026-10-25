use super::*;

use bollard::models::SystemInfo;

use crate::launch::resource_probe::provider::{ImageProvider, VerifiedCandidate};

struct FixtureProvider;

impl ImageProvider for FixtureProvider {
    async fn verified_candidate(
        &self,
        _docker: &bollard::Docker,
        _journal: &Journal,
        _engine_id: &str,
        _info: &SystemInfo,
    ) -> Result<Option<VerifiedCandidate>, HostError> {
        std::future::ready(()).await;
        Ok(Some(VerifiedCandidate {
            runtime_id: RUNTIME_ID.to_owned(),
            platform: "linux/amd64".to_owned(),
            daemon_architecture: "x86_64".to_owned(),
            source_revision: REVISION.to_owned(),
            binding_fingerprint: BINDING.to_owned(),
            config: expected_image_config(REVISION),
            engine_id: ENGINE_ID.to_owned(),
            docker_root_digest: super::super::super::docker_root_digest("/var/lib/docker")
                .expect("test root digest"),
            workspace_cleanup_digest: "d".repeat(64),
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
    super::super::configure_probe_capacity(&journal).await?;
    let info_a = r#"{"ID":"probe-engine","DockerRootDir":"/var/lib/docker","OSType":"linux","Architecture":"x86_64","NCPU":8,"MemTotal":17179869184}"#;
    let info_b = r#"{"ID":"other-engine","DockerRootDir":"/var/lib/docker","OSType":"linux","Architecture":"x86_64","NCPU":8,"MemTotal":17179869184}"#;
    let docker = DockerStub::open(vec![
        http(200, info_a),
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
    assert_request_order(&requests, &[RequestKind::Info; 4])?;
    assert_eq!(requests.len(), 4);
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
async fn root_change_before_create_keeps_uncertain_create_charged_after_absence()
-> Result<(), String> {
    let scratch = Scratch::new("probe-root-before-create").map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_engine(ENGINE_ID)
        .await
        .map_err(|error| error.to_string())?;
    super::super::configure_probe_capacity(&journal).await?;
    let original = r#"{"ID":"probe-engine","DockerRootDir":"/var/lib/docker","OSType":"linux","Architecture":"x86_64","NCPU":8,"MemTotal":17179869184}"#;
    let replacement = r#"{"ID":"probe-engine","DockerRootDir":"/var/lib/docker-next","OSType":"linux","Architecture":"x86_64","NCPU":8,"MemTotal":17179869184}"#;
    let changed = DockerStub::open(vec![
        http(200, original),
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
    assert_request_order(&changed_requests, &[RequestKind::Info; 4])?;
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
    assert!(matches!(result, Err(HostError::Ownership)));
    assert_request_order(
        &recovery_requests,
        &[RequestKind::Info, RequestKind::Info, RequestKind::Inspect],
    )?;
    assert_eq!(recovery_requests.len(), 3);
    let retained = journal
        .active_probe()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| {
            "uncertain create row was discarded after one absent-name read".to_owned()
        })?;
    assert_eq!(retained.phase, crate::journal::ProbePhase::Quarantined);
    assert_eq!(
        retained.lease_state,
        crate::journal::ProbeLeaseState::Uncertain
    );
    assert!(retained.container_id.is_none());
    Ok(())
}

#[tokio::test]
async fn engine_change_after_create_blocks_start_then_cleans_on_bound_engine() -> Result<(), String>
{
    let (_scratch, journal, projection) = prepared().await?;
    let info_a = r#"{"ID":"probe-engine","DockerRootDir":"/var/lib/docker","OSType":"linux","Architecture":"x86_64"}"#;
    let info_a_full = r#"{"ID":"probe-engine","DockerRootDir":"/var/lib/docker","OSType":"linux","Architecture":"x86_64","NCPU":8,"MemTotal":17179869184}"#;
    let info_b = r#"{"ID":"other-engine","DockerRootDir":"/var/lib/docker","OSType":"linux","Architecture":"x86_64"}"#;
    let changed = DockerStub::open(vec![
        http(200, info_a),
        http(200, &format!(r#"{{"Id":"{CONTAINER_ID}","Warnings":[]}}"#)),
        http(200, info_a),
        http(200, &inspect(&projection, "created", 0)?),
        http(200, &inspect(&projection, "created", 0)?),
        http(200, info_b),
        http(200, info_b),
    ])?;
    let result = Box::pin(execute::run(
        &changed.docker,
        &journal,
        &projection,
        image_evidence(&projection),
        16 * 1024 * 1024 * 1024,
        &Deadline::new(),
    ))
    .await;
    let changed_requests = changed.finish_observed().await?;
    assert!(matches!(result, Err(HostError::Identity)));
    assert_start_rebind_blocks_effects(&changed_requests)?;
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
    assert_recovered_container_cleanup(&retry_requests)?;
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
    let original = r#"{"ID":"probe-engine","DockerRootDir":"/var/lib/docker","OSType":"linux","Architecture":"x86_64"}"#;
    let original_full = r#"{"ID":"probe-engine","DockerRootDir":"/var/lib/docker","OSType":"linux","Architecture":"x86_64","NCPU":8,"MemTotal":17179869184}"#;
    let replacement = r#"{"ID":"probe-engine","DockerRootDir":"/var/lib/docker-next","OSType":"linux","Architecture":"x86_64"}"#;
    let changed = DockerStub::open(vec![
        http(200, original),
        http(200, &format!(r#"{{"Id":"{CONTAINER_ID}","Warnings":[]}}"#)),
        http(200, original),
        http(200, &inspect(&projection, "created", 0)?),
        http(200, &inspect(&projection, "created", 0)?),
        http(200, replacement),
        http(200, replacement),
    ])?;
    let result = Box::pin(execute::run(
        &changed.docker,
        &journal,
        &projection,
        image_evidence(&projection),
        16 * 1024 * 1024 * 1024,
        &Deadline::new(),
    ))
    .await;
    let changed_requests = changed.finish_observed().await?;
    assert!(matches!(result, Err(HostError::Identity)));
    assert_start_rebind_blocks_effects(&changed_requests)?;
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
    assert_recovered_container_cleanup(&recovery_requests)?;
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
