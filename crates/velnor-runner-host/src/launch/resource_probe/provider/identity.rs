// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use bollard::Docker;
use bollard::models::SystemInfo;

use crate::launch::resource_probe::projection::DockerRoot;

use super::{ProviderFailure, valid_sha};

pub(super) struct SelectedDaemon {
    pub(super) engine_id: String,
    pub(super) docker_root_digest: String,
    pub(super) architecture: String,
}

pub(super) fn selected_daemon(
    info: &SystemInfo,
    engine_id: &str,
) -> Result<SelectedDaemon, ProviderFailure> {
    if info.id.as_deref() != Some(engine_id) || !daemon_platform_matches(info) {
        return Err(ProviderFailure::ImageIdentity);
    }
    let root = info
        .docker_root_dir
        .as_deref()
        .ok_or(ProviderFailure::ImageIdentity)?;
    let root = DockerRoot::parse(root).map_err(|_| ProviderFailure::ImageIdentity)?;
    let architecture = daemon_architecture(info).ok_or(ProviderFailure::ImageIdentity)?;
    Ok(SelectedDaemon {
        engine_id: engine_id.to_owned(),
        docker_root_digest: root.digest().to_owned(),
        architecture: architecture.to_owned(),
    })
}

pub(super) async fn verify_selected_daemon(
    docker: &Docker,
    selected: &SelectedDaemon,
) -> Result<(), ProviderFailure> {
    selected_daemon_info(docker, selected).await.map(|_| ())
}

pub(super) async fn selected_daemon_info(
    docker: &Docker,
    selected: &SelectedDaemon,
) -> Result<SelectedDaemon, ProviderFailure> {
    let info = tokio::time::timeout(Duration::from_secs(20), docker.info())
        .await
        .map_err(|_| ProviderFailure::DaemonIdentity)?
        .map_err(|_| ProviderFailure::DaemonIdentity)?;
    let current = selected_daemon(&info, &selected.engine_id)?;
    if current.engine_id == selected.engine_id
        && current.docker_root_digest == selected.docker_root_digest
        && current.architecture == selected.architecture
    {
        Ok(current)
    } else {
        Err(ProviderFailure::ImageIdentity)
    }
}

pub(super) fn daemon_platform_matches(info: &SystemInfo) -> bool {
    info.os_type.as_deref() == Some("linux") && daemon_architecture(info).is_some()
}

pub(super) fn daemon_architecture(info: &SystemInfo) -> Option<&'static str> {
    match info.architecture.as_deref()? {
        "x86_64" | "amd64" => Some("x86_64"),
        "aarch64" | "arm64" => Some("aarch64"),
        _ => None,
    }
}

pub(super) fn compiled_source_revision() -> Result<String, ProviderFailure> {
    let identity = crate::compile_identity::compiled_release_identity()
        .ok_or(ProviderFailure::TrustedSourcePinMissing)?;
    let source_revision = super::hex(identity.source_sha());
    if valid_sha(&source_revision) {
        Ok(source_revision)
    } else {
        Err(ProviderFailure::TrustedSourcePinInvalid)
    }
}

pub(super) fn build_pin_status() -> Result<(), &'static str> {
    compiled_source_revision()
        .map(|_| ())
        .map_err(ProviderFailure::code)
}
