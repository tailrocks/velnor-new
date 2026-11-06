// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use bollard::Docker;
use bollard::models::{ContainerConfig, ImageInspect, SystemInfo};
use bollard::query_parameters::CreateImageOptionsBuilder;
use futures_util::StreamExt;
use tokio::time::{Instant, timeout_at};

use crate::error::HostError;
use crate::resource_probe_release::runner_manifest::model::ImageEntry;

use super::RuntimeImage;
use super::hash::is_sha256_digest;

const MAX_PULL_MESSAGES: usize = 16_384;

pub(super) async fn pull_and_inspect(
    docker: &Docker,
    image: &ImageEntry,
    source: &str,
    role: &str,
    engine_id: &str,
    root: &str,
    deadline: Instant,
) -> Result<RuntimeImage, HostError> {
    ensure_engine(docker, engine_id, root, deadline).await?;
    let reference = format!("{}@{}", image.repository, image.registry_digest);
    let options = CreateImageOptionsBuilder::new()
        .from_image(&reference)
        .platform("linux/amd64")
        .build();
    let pull = async {
        let mut stream = docker.create_image(Some(options), None, None);
        let mut messages = 0_usize;
        while let Some(item) = stream.next().await {
            messages = messages
                .checked_add(1)
                .filter(|count| *count <= MAX_PULL_MESSAGES)
                .ok_or(HostError::Frame)?;
            let item = item.map_err(|_| HostError::Docker)?;
            if item.error_detail.is_some() {
                return Err(HostError::Docker);
            }
        }
        Ok::<(), HostError>(())
    };
    timeout_at(deadline, pull)
        .await
        .map_err(|_| HostError::DockerTimeout)??;
    ensure_engine(docker, engine_id, root, deadline).await?;
    let inspect = timeout_at(deadline, inspect_for_platform(docker, &reference))
        .await
        .map_err(|_| HostError::DockerTimeout)??;
    let runtime = validate_inspect(inspect, image, source, role, &reference)?;
    ensure_engine(docker, engine_id, root, deadline).await?;
    Ok(runtime)
}

async fn inspect_for_platform(docker: &Docker, reference: &str) -> Result<ImageInspect, HostError> {
    let version = docker.client_version();
    if (version.major_version, version.minor_version) < (1, 49) {
        return Err(HostError::Identity);
    }
    let expected = format!("/images/{reference}/json");
    let versioned = format!(
        "/v{}.{}{}",
        version.major_version, version.minor_version, expected
    );
    let applied = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&applied);
    let platform_docker = docker.clone().with_request_modifier(move |mut request| {
        if request.method().as_str() != "GET" {
            return request;
        }
        let mut parts = request.uri().clone().into_parts();
        let Some(path) = parts.path_and_query.as_ref() else {
            return request;
        };
        if path.path() != expected || path.query().is_some() {
            return request;
        }
        let Ok(path) = format!(
            "{versioned}?platform=%7B%22architecture%22%3A%22amd64%22%2C%22os%22%3A%22linux%22%7D"
        )
        .parse() else {
            return request;
        };
        parts.path_and_query = Some(path);
        let Ok(uri) = parts.try_into() else {
            return request;
        };
        *request.uri_mut() = uri;
        flag.store(true, Ordering::Release);
        request
    });
    let response = platform_docker
        .inspect_image(reference)
        .await
        .map_err(|_| HostError::Docker)?;
    if !applied.load(Ordering::Acquire) {
        return Err(HostError::Identity);
    }
    Ok(response)
}

fn validate_inspect(
    inspect: ImageInspect,
    image: &ImageEntry,
    source: &str,
    role: &str,
    reference: &str,
) -> Result<RuntimeImage, HostError> {
    if inspect.os.as_deref() != Some("linux")
        || inspect.architecture.as_deref() != Some("amd64")
        || inspect
            .variant
            .as_deref()
            .is_some_and(|variant| !variant.is_empty())
        || !inspect
            .repo_digests
            .as_ref()
            .is_some_and(|digests| digests.iter().any(|digest| digest == reference))
    {
        return Err(HostError::Identity);
    }
    let runtime_id = inspect.id.ok_or(HostError::Identity)?;
    if !is_sha256_digest(&runtime_id) {
        return Err(HostError::Identity);
    }
    match inspect.descriptor {
        Some(descriptor) => {
            if descriptor.digest.as_deref() != Some(image.platform_manifest_digest.as_str())
                || runtime_id != image.platform_manifest_digest
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
        None if runtime_id != image.config_digest => return Err(HostError::Identity),
        None => {}
    }
    let config = inspect.config.ok_or(HostError::Identity)?;
    let revision = config
        .labels
        .as_ref()
        .and_then(|labels| labels.get("org.opencontainers.image.revision"));
    if revision.map(String::as_str) != Some(source) {
        return Err(HostError::Identity);
    }
    let kind = config
        .labels
        .as_ref()
        .and_then(|labels| labels.get("io.velnor.image.kind"));
    let (user, entrypoint) = match role {
        "runner" => ("runner", "/usr/local/bin/velnor-runner-entrypoint"),
        "dind" => ("", "/usr/local/bin/velnor-dind-entrypoint"),
        "resource-probe" => ("65532:65532", "/velnor/resource-probe"),
        _ => return Err(HostError::Identity),
    };
    if config.user.as_deref().unwrap_or("") != user
        || config.entrypoint.as_deref() != Some(&[entrypoint.to_owned()][..])
        || (role != "resource-probe" && kind.map(String::as_str) != Some(role))
        || (role == "resource-probe" && config.working_dir.as_deref() != Some("/"))
    {
        return Err(HostError::Identity);
    }
    let value = serde_json::to_value(config).map_err(|_| HostError::Identity)?;
    let config: ContainerConfig = serde_json::from_value(value).map_err(|_| HostError::Identity)?;
    Ok(RuntimeImage { runtime_id, config })
}

async fn ensure_engine(
    docker: &Docker,
    id: &str,
    root: &str,
    deadline: Instant,
) -> Result<(), HostError> {
    let info: SystemInfo = timeout_at(deadline, docker.info())
        .await
        .map_err(|_| HostError::DockerTimeout)?
        .map_err(|_| HostError::Docker)?;
    if info.id.as_deref() != Some(id) || info.docker_root_dir.as_deref() != Some(root) {
        return Err(HostError::Identity);
    }
    Ok(())
}
