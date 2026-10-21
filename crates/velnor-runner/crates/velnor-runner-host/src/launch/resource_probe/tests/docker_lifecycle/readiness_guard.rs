//! Test readiness sampling when another durable probe is active.

use super::{DockerStub, HostError, UnavailableImageProvider, prepared};
use crate::launch::resource_probe::readiness;

#[tokio::test]
async fn readiness_sampler_leaves_an_active_prepared_probe_untouched() -> Result<(), String> {
    let (_scratch, journal, _projection) = prepared().await?;
    let before = journal
        .active_probe()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The prepared probe fixture is missing.".to_owned())?;
    let docker = DockerStub::open(Vec::new())?;
    let observation =
        readiness::collect_with_provider(&docker.docker, &journal, &UnavailableImageProvider)
            .await
            .map_err(|error| error.to_string())?;
    assert!(observation.is_none());
    assert!(docker.finish_observed().await?.is_empty());
    let after = journal
        .active_probe()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Readiness sampling removed the active probe.".to_owned())?;
    assert_eq!(after, before);
    Ok(())
}

#[tokio::test]
async fn readiness_collect_rejects_missing_release_identity_without_docker_or_capacity_release()
-> Result<(), String> {
    if crate::compile_identity::compiled_release_identity().is_some() {
        return Err("The unit test binary must not have a release identity.".to_owned());
    }
    let (_scratch, journal, _projection) = prepared().await?;
    let before_probe = journal
        .active_probe()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The prepared probe fixture is missing.".to_owned())?;
    let before_capacity = journal
        .readiness_capacity_usage()
        .await
        .map_err(|error| error.to_string())?;
    let before_revision = journal
        .revision()
        .await
        .map_err(|error| error.to_string())?;
    let docker = DockerStub::open(Vec::new())?;

    let result = readiness::collect(
        &docker.docker,
        &journal,
        &crate::launch::ReleaseReadSecret::test_token(),
    )
    .await;

    assert!(matches!(result, Err(HostError::Docker)));
    assert!(docker.finish_observed().await?.is_empty());
    assert_eq!(
        journal
            .active_probe()
            .await
            .map_err(|error| error.to_string())?,
        Some(before_probe)
    );
    assert_eq!(
        journal
            .readiness_capacity_usage()
            .await
            .map_err(|error| error.to_string())?,
        before_capacity
    );
    assert_eq!(
        journal
            .revision()
            .await
            .map_err(|error| error.to_string())?,
        before_revision
    );
    Ok(())
}
