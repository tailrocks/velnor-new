// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::config::RunnerPlatform;
use crate::error::HostError;
use crate::journal::EngineContext;
use bollard::models::SystemInfo;

use super::VerifiedWorkerImages;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SelectedEngineContext {
    engine_id: String,
    endpoint: String,
    platform: String,
    docker_root_digest: String,
    daemon_architecture: String,
}

impl SelectedEngineContext {
    pub(crate) fn capture(selected: &EngineContext, info: &SystemInfo) -> Result<Self, HostError> {
        let engine_id = info.id.as_deref().ok_or(HostError::Ownership)?;
        let docker_root = info
            .docker_root_dir
            .as_deref()
            .ok_or(HostError::Ownership)?;
        let docker_root_digest = super::resource_probe::docker_root_digest(docker_root)?;
        let daemon_architecture =
            super::resource_probe::daemon_architecture(info).ok_or(HostError::Ownership)?;
        if !super::resource_probe::daemon_platform_matches(info)
            || !runs_on(&selected.platform, daemon_architecture)
            || selected.engine_id != engine_id
            || selected.endpoint.is_empty()
        {
            return Err(HostError::Ownership);
        }
        Ok(Self {
            engine_id: selected.engine_id.clone(),
            endpoint: selected.endpoint.clone(),
            platform: selected.platform.clone(),
            docker_root_digest,
            daemon_architecture: daemon_architecture.to_owned(),
        })
    }

    #[must_use]
    pub(crate) fn matches_images(&self, images: &VerifiedWorkerImages) -> bool {
        self.engine_id == images.engine_id()
            && self.platform == images.platform()
            && self.docker_root_digest == images.docker_root_digest()
            && self.daemon_architecture == images.daemon_architecture()
    }

    #[must_use]
    pub(crate) fn matches_selection(&self, selected: &EngineContext) -> bool {
        self.engine_id == selected.engine_id
            && self.endpoint == selected.endpoint
            && self.platform == selected.platform
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub(crate) fn platform(&self) -> &str {
        &self.platform
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }

    pub(crate) fn daemon_architecture(&self) -> &str {
        &self.daemon_architecture
    }
}

fn runs_on(platform: &str, daemon_architecture: &str) -> bool {
    match RunnerPlatform::parse(platform) {
        Some(RunnerPlatform::LinuxAmd64) => true,
        Some(platform) => platform.daemon_architecture() == daemon_architecture,
        None => false,
    }
}

#[cfg(test)]
mod tests;
