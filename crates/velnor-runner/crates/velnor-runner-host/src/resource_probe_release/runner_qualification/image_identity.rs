// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::time::Duration;

use crate::launch::ProjectedCandidate;
use bollard::Docker;
use bollard::models::{ImageInspect, SystemInfo, SystemVersion};

use crate::error::HostError;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct DaemonEvidence {
    id: String,
    pub(super) root: String,
    version: String,
    api_version: String,
    git_commit: String,
    os: String,
    arch: String,
}

impl DaemonEvidence {
    pub(super) fn report(&self) {
        println!(
            "docker_version={} docker_api_version={} docker_git_commit={} docker_os={} docker_arch={} docker_id={} docker_root={} docker_store=io.containerd.snapshotter.v1",
            self.version, self.api_version, self.git_commit, self.os, self.arch, self.id, self.root
        );
    }
}

pub(super) async fn daemon(docker: &Docker) -> Result<DaemonEvidence, HostError> {
    let version = super::deadline(docker.version(), Duration::from_secs(5))
        .await?
        .map_err(|_| HostError::Docker)?;
    let info = super::deadline(docker.info(), Duration::from_secs(5))
        .await?
        .map_err(|_| HostError::Docker)?;
    validate_daemon(&version, &info)
}

fn validate_daemon(
    version: &SystemVersion,
    info: &SystemInfo,
) -> Result<DaemonEvidence, HostError> {
    if version.os.as_deref() != Some("linux") {
        return Err(HostError::Identity);
    }
    if !is_containerd(info) {
        return Err(HostError::Identity);
    }
    Ok(DaemonEvidence {
        id: required(info.id.as_deref())?,
        root: required(info.docker_root_dir.as_deref())?,
        version: required(version.version.as_deref())?,
        api_version: required(version.api_version.as_deref())?,
        git_commit: required(version.git_commit.as_deref())?,
        os: required(version.os.as_deref())?,
        arch: required(version.arch.as_deref())?,
    })
}

fn is_containerd(info: &SystemInfo) -> bool {
    let Some(status) = info.driver_status.as_ref() else {
        return false;
    };
    let mut drivers = status
        .iter()
        .filter(|pair| pair.first().is_some_and(|key| key == "driver-type"));
    drivers
        .next()
        .is_some_and(|driver| driver.len() == 2 && driver[1] == "io.containerd.snapshotter.v1")
        && drivers.next().is_none()
}

fn required(value: Option<&str>) -> Result<String, HostError> {
    value
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or(HostError::Identity)
}

pub(super) async fn inspect(
    docker: &Docker,
    expected: &ProjectedCandidate,
) -> Result<(), HostError> {
    let image = super::deadline(
        docker.inspect_image(&expected.oci.platform_manifest_digest),
        Duration::from_secs(5),
    )
    .await?
    .map_err(|_| HostError::Docker)?;
    validate_image(&image, expected)
}

fn validate_image(image: &ImageInspect, expected: &ProjectedCandidate) -> Result<(), HostError> {
    let descriptor = image.descriptor.as_ref().ok_or(HostError::Identity)?;
    if image.id.as_deref() != Some(expected.oci.platform_manifest_digest.as_str())
        || descriptor.digest.as_deref() != Some(expected.oci.platform_manifest_digest.as_str())
        || descriptor.media_type.as_deref() != Some("application/vnd.oci.image.manifest.v1+json")
        || descriptor.size.and_then(|size| u64::try_from(size).ok())
            != Some(expected.oci.platform_manifest_size_bytes)
        || image.os.as_deref() != Some("linux")
        || image.architecture.as_deref() != Some("amd64")
        || image
            .os_version
            .as_deref()
            .is_some_and(|value| !value.is_empty())
        || image
            .variant
            .as_deref()
            .is_some_and(|variant| !variant.is_empty())
    {
        return Err(HostError::Identity);
    }
    if descriptor.platform.as_ref().is_some_and(|platform| {
        platform.os.as_deref() != Some("linux")
            || platform.architecture.as_deref() != Some("amd64")
            || platform
                .os_version
                .as_deref()
                .is_some_and(|value| !value.is_empty())
            || platform
                .os_features
                .as_ref()
                .is_some_and(|features| !features.is_empty())
            || platform
                .variant
                .as_deref()
                .is_some_and(|variant| !variant.is_empty())
    }) {
        return Err(HostError::Identity);
    }
    let rootfs = image.root_fs.as_ref().ok_or(HostError::Identity)?;
    if rootfs.typ != "layers" || rootfs.layers.as_deref() != Some(expected.diff_ids.as_slice()) {
        return Err(HostError::Identity);
    }
    let config = image.config.as_ref().ok_or(HostError::Identity)?;
    let source_healthcheck = source_healthcheck(&expected.configuration)?;
    let admitted: bollard::models::ImageConfig =
        serde_json::from_value(expected.configuration.clone()).map_err(|_| HostError::Identity)?;
    let labels = config.labels.as_ref().map(|labels| {
        labels
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<BTreeMap<_, _>>()
    });
    let admitted_labels = admitted.labels.as_ref().map(|labels| {
        labels
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<BTreeMap<_, _>>()
    });
    if !same_healthcheck(config.healthcheck.as_ref(), source_healthcheck.as_ref())
        || config
            .volumes
            .as_ref()
            .is_some_and(|volumes| !volumes.is_empty())
        || labels != admitted_labels
        || config.user != admitted.user
        || config.working_dir != admitted.working_dir
        || config.entrypoint != admitted.entrypoint
        || config.cmd != admitted.cmd
        || config.env != admitted.env
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

pub(super) fn healthcheck_disabled(healthcheck: Option<&bollard::models::HealthConfig>) -> bool {
    healthcheck.is_none_or(|healthcheck| {
        healthcheck
            .test
            .as_deref()
            .is_some_and(|test| test.len() == 1 && test[0] == "NONE")
    })
}

pub(crate) fn source_healthcheck(
    expected: &serde_json::Value,
) -> Result<Option<bollard::models::HealthConfig>, HostError> {
    let mut projection = serde_json::Map::new();
    if let Some(healthcheck) = expected.get("Healthcheck") {
        if let Some(object) = healthcheck.as_object() {
            const KEYS: [&str; 6] = [
                "Test",
                "Interval",
                "Timeout",
                "Retries",
                "StartPeriod",
                "StartInterval",
            ];
            if object.keys().any(|key| !KEYS.contains(&key.as_str())) {
                return Err(HostError::Identity);
            }
        }
        projection.insert("Healthcheck".to_owned(), healthcheck.clone());
    }
    if let Some(volumes) = expected.get("Volumes") {
        projection.insert("Volumes".to_owned(), volumes.clone());
    }
    let config: bollard::models::ImageConfig =
        serde_json::from_value(serde_json::Value::Object(projection))
            .map_err(|_| HostError::Identity)?;
    if !healthcheck_disabled(config.healthcheck.as_ref())
        || config
            .volumes
            .as_ref()
            .is_some_and(|volumes| !volumes.is_empty())
    {
        return Err(HostError::Identity);
    }
    Ok(config.healthcheck)
}

pub(crate) fn same_healthcheck(
    left: Option<&bollard::models::HealthConfig>,
    right: Option<&bollard::models::HealthConfig>,
) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => {
            left.test == right.test
                && left.interval.unwrap_or_default() == right.interval.unwrap_or_default()
                && left.timeout.unwrap_or_default() == right.timeout.unwrap_or_default()
                && left.retries.unwrap_or_default() == right.retries.unwrap_or_default()
                && left.start_period.unwrap_or_default() == right.start_period.unwrap_or_default()
                && left.start_interval.unwrap_or_default()
                    == right.start_interval.unwrap_or_default()
        }
        _ => false,
    }
}
