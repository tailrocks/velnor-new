// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use bollard::{Docker, models::ContainerConfig, models::SystemInfo};

use crate::error::HostError;

pub(super) struct VerifiedCandidate {
    pub(super) runtime_id: String,
    pub(super) platform: String,
    pub(super) source_revision: String,
    pub(super) binding_fingerprint: String,
    pub(super) config: ContainerConfig,
}

pub(super) trait ImageProvider {
    async fn verified_candidate(
        &self,
        docker: &Docker,
        engine_id: &str,
        info: &SystemInfo,
    ) -> Result<Option<VerifiedCandidate>, HostError>;
}
