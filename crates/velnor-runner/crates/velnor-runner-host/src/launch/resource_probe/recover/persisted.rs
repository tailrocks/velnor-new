// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;
use crate::journal::{HostHelperRole, ProbeRow, ProbeWork};

use super::super::projection::{DockerRoot, ProbeProjection, VerifiedProbeImage};

pub(super) fn persisted_projection(
    row: &ProbeRow,
    root: &DockerRoot,
) -> Result<ProbeProjection, HostError> {
    let ProbeWork::ResourceProbe {
        purpose,
        request_sha256,
        ..
    } = row.work()
    else {
        return Err(HostError::Ownership);
    };
    let mut matched = None;
    for (platform, daemon_architecture) in [
        crate::config::RunnerPlatform::LinuxAmd64,
        crate::config::RunnerPlatform::LinuxArm64,
    ]
    .into_iter()
    .flat_map(|platform| ["x86_64", "aarch64"].map(|architecture| (platform, architecture)))
    {
        let image = VerifiedProbeImage::from_journal(
            row.runtime_image_id.clone(),
            platform,
            daemon_architecture.to_owned(),
            row.source_revision.clone(),
            row.image_binding_digest.clone(),
        )?;
        let projection = ProbeProjection::build_from_purpose(
            row.operation_id.clone(),
            row.instance_id.clone(),
            row.engine_id.clone(),
            root.path(),
            image,
            *purpose,
            request_sha256.clone(),
        )?;
        if row.projection_digest == projection.projection_digest
            && matched.replace(projection).is_some()
        {
            return Err(HostError::Ownership);
        }
    }
    let projection = matched.ok_or(HostError::Ownership)?;
    if row.helper_role() != HostHelperRole::ResourceProbe
        || row.role != "resource-probe"
        || row.operation_name != projection.name
        || row.projection_digest != projection.projection_digest
    {
        return Err(HostError::Ownership);
    }
    Ok(projection)
}
