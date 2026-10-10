//! Offline image import and exact daemon-local identity validation.

use std::time::Duration;

use bollard::Docker;
use bollard::models::{ContainerConfig, ImageInspect, SystemInfo};
use bollard::query_parameters::ImportImageOptionsBuilder;
use futures_util::StreamExt;
use tokio::time::{Instant, timeout_at};

use crate::error::HostError;

use super::RuntimeImage;
use super::archive::ArchiveIdentity;
use super::manifest::{PLATFORM, ProbeManifest};
use super::release::VerifiedRelease;

const MINIMUM_LOAD_WINDOW: Duration = Duration::from_secs(5);
const MAX_LOAD_MESSAGES: usize = 1024;

pub(super) async fn load_and_inspect(
    docker: &Docker,
    engine_id: &str,
    docker_root: &str,
    release: &VerifiedRelease,
    deadline: Instant,
) -> Result<RuntimeImage, HostError> {
    ensure_remaining(deadline, Duration::ZERO)?;
    ensure_engine(docker, engine_id, docker_root, deadline).await?;
    if let Some(runtime) = inspect(docker, release, deadline).await? {
        ensure_engine(docker, engine_id, docker_root, deadline).await?;
        return Ok(runtime);
    }
    ensure_remaining(deadline, MINIMUM_LOAD_WINDOW)?;
    ensure_engine(docker, engine_id, docker_root, deadline).await?;
    load_archive(docker, &release.archive, deadline).await?;
    ensure_engine(docker, engine_id, docker_root, deadline).await?;
    let runtime = inspect(docker, release, deadline)
        .await?
        .ok_or(HostError::Identity)?;
    ensure_engine(docker, engine_id, docker_root, deadline).await?;
    Ok(runtime)
}

async fn load_archive(docker: &Docker, archive: &[u8], deadline: Instant) -> Result<(), HostError> {
    ensure_remaining(deadline, MINIMUM_LOAD_WINDOW)?;
    let load = async {
        let mut stream = docker.import_image(
            ImportImageOptionsBuilder::new().quiet(true).build(),
            bollard::body_full(archive.to_vec().into()),
            None,
        );
        let mut count = 0_usize;
        while let Some(response) = stream.next().await {
            count = count.checked_add(1).ok_or(HostError::Frame)?;
            if count > MAX_LOAD_MESSAGES {
                return Err(HostError::Frame);
            }
            let response = response.map_err(|_| HostError::Docker)?;
            if response.error_detail.is_some() {
                return Err(HostError::Docker);
            }
        }
        Ok(())
    };
    timeout_at(deadline, load)
        .await
        .map_err(|_| HostError::DockerTimeout)??;
    Ok(())
}

async fn inspect(
    docker: &Docker,
    release: &VerifiedRelease,
    deadline: Instant,
) -> Result<Option<RuntimeImage>, HostError> {
    let response = timeout_at(deadline, docker.inspect_image(super::manifest::IMAGE_TAG))
        .await
        .map_err(|_| HostError::DockerTimeout)?;
    match response {
        Ok(inspect) => {
            validate_inspect(inspect, &release.manifest, &release.archive_identity).map(Some)
        }
        Err(error) if is_image_missing(&error) => Ok(None),
        Err(_) => Err(HostError::Docker),
    }
}

fn is_image_missing(error: &bollard::errors::Error) -> bool {
    matches!(
        error,
        bollard::errors::Error::DockerResponseServerError {
            status_code: 404,
            ..
        }
    )
}

fn validate_inspect(
    inspect: ImageInspect,
    manifest: &ProbeManifest,
    identity: &ArchiveIdentity,
) -> Result<RuntimeImage, HostError> {
    if inspect.os.as_deref() != Some("linux")
        || inspect.architecture.as_deref() != Some("amd64")
        || inspect.variant.is_some()
    {
        return Err(HostError::Identity);
    }
    let runtime_id = inspect.id.ok_or(HostError::Identity)?;
    if !super::hash::is_sha256_digest(&runtime_id) {
        return Err(HostError::Identity);
    }
    match inspect.descriptor {
        Some(descriptor) => {
            let size =
                i64::try_from(identity.image_manifest_size).map_err(|_| HostError::Identity)?;
            if descriptor.media_type.as_deref() != Some(identity.image_manifest_media_type.as_str())
                || descriptor.digest.as_deref() != Some(identity.image_manifest_digest.as_str())
                || descriptor.size.is_some_and(|value| value != size)
                || runtime_id != identity.image_manifest_digest
            {
                return Err(HostError::Identity);
            }
            if let Some(platform) = descriptor.platform
                && (platform.os.as_deref() != Some("linux")
                    || platform.architecture.as_deref() != Some("amd64")
                    || platform.variant.is_some()
                    || platform.os_version.is_some()
                    || platform.os_features.is_some())
            {
                return Err(HostError::Identity);
            }
        }
        None if runtime_id != identity.config_digest => return Err(HostError::Identity),
        None => {}
    }
    let config = inspect.config.ok_or(HostError::Identity)?;
    let value = serde_json::to_value(config).map_err(|_| HostError::Identity)?;
    let config: ContainerConfig = serde_json::from_value(value).map_err(|_| HostError::Identity)?;
    if config != identity.config || manifest.platform != PLATFORM {
        return Err(HostError::Identity);
    }
    Ok(RuntimeImage { runtime_id, config })
}

async fn ensure_engine(
    docker: &Docker,
    expected_engine: &str,
    expected_root: &str,
    deadline: Instant,
) -> Result<(), HostError> {
    let info: SystemInfo = timeout_at(deadline, docker.info())
        .await
        .map_err(|_| HostError::DockerTimeout)?
        .map_err(|_| HostError::Docker)?;
    if info.id.as_deref() != Some(expected_engine)
        || info.docker_root_dir.as_deref() != Some(expected_root)
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

fn ensure_remaining(deadline: Instant, minimum: Duration) -> Result<(), HostError> {
    if deadline.saturating_duration_since(Instant::now()) > minimum {
        Ok(())
    } else {
        Err(HostError::DockerTimeout)
    }
}

#[cfg(test)]
mod tests {
    use super::is_image_missing;

    #[test]
    fn only_exact_docker_not_found_allows_an_import_attempt() {
        let not_found = bollard::errors::Error::DockerResponseServerError {
            status_code: 404,
            message: "not found".to_owned(),
        };
        let server_error = bollard::errors::Error::DockerResponseServerError {
            status_code: 500,
            message: "server error".to_owned(),
        };

        assert!(is_image_missing(&not_found));
        assert!(!is_image_missing(&server_error));
    }
}
