// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Serialize;

use crate::config::{RepositoryConnection, RunnerPlatform};
use crate::error::HostError;
use crate::launch::verified_images::WorkerImageOrigin;
use velnor_runner_image_manifest::ImageKind;

#[path = "identity.rs"]
mod identity;
use crate::launch::{VerifiedImageIdentity, VerifiedWorkerImages};
use identity::GuestImageIdentity;

const SCHEMA: &str = "velnor.guest-image/v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct GuestImageDocument {
    schema: &'static str,
    source_sha: String,
    guest_platform: &'static str,
    scale_set: GuestScaleSet,
    images: GuestImages,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct GuestImages {
    runner: GuestImageIdentity,
    dind: GuestImageIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct GuestScaleSet {
    name: String,
    labels: Vec<String>,
}

impl GuestImageDocument {
    pub(super) fn new(
        connection: &RepositoryConnection,
        images: &VerifiedWorkerImages,
    ) -> Result<Self, HostError> {
        let platform = Self::validate_images(images)?;
        let selector = &connection.scale_set;
        let canonical_selector =
            crate::config::ScaleSetSelector::try_new(selector.name(), selector.labels())?;
        if canonical_selector != *selector {
            return Err(HostError::ForbiddenMount);
        }
        Ok(Self {
            schema: SCHEMA,
            source_sha: images.source_sha().to_owned(),
            guest_platform: platform.as_str(),
            scale_set: GuestScaleSet {
                name: selector.name().to_owned(),
                labels: selector.labels().to_vec(),
            },
            images: GuestImages {
                runner: guest_identity(images.runner(), ImageKind::Runner, images)?,
                dind: guest_identity(images.dind(), ImageKind::Dind, images)?,
            },
        })
    }

    pub(super) fn validate_images(
        images: &VerifiedWorkerImages,
    ) -> Result<RunnerPlatform, HostError> {
        let platform = RunnerPlatform::parse(images.platform()).ok_or(HostError::ForbiddenMount)?;
        if images.engine_id().is_empty()
            || !lower_hex(images.docker_root_digest(), 64)
            || images.daemon_architecture().is_empty()
            || !lower_hex(images.source_sha(), 40)
        {
            return Err(HostError::ForbiddenMount);
        }
        guest_identity(images.runner(), ImageKind::Runner, images)?;
        guest_identity(images.dind(), ImageKind::Dind, images)?;
        Ok(platform)
    }
}

fn guest_identity(
    image: &VerifiedImageIdentity,
    role: ImageKind,
    images: &VerifiedWorkerImages,
) -> Result<GuestImageIdentity, HostError> {
    if !sha256_digest(image.runtime_id()) {
        return Err(HostError::ForbiddenMount);
    }
    let result = match image.origin() {
        WorkerImageOrigin::Registry(registry)
            if registry.observation.validate(&registry.identity).is_ok() =>
        {
            let expected_manifest = images
                .registry_manifest_sha256()
                .ok_or(HostError::ForbiddenMount)?;
            identity::registry(&identity::RegistryInput {
                role: registry.identity.role,
                expected_role: role,
                source: &registry.identity.source_sha,
                expected_source: images.source_sha(),
                release_manifest_sha256: &registry.identity.release_manifest_sha256,
                expected_release_manifest_sha256: expected_manifest,
                registry_digest: &registry.identity.registry_digest,
                manifest_digest: &registry.identity.registry_digest,
                config_digest: &registry.identity.config_digest,
            })
        }
        WorkerImageOrigin::Registry(_) => None,
        WorkerImageOrigin::Developer(developer) => {
            identity::developer(image.runtime_id(), &developer.descriptor_digest)
        }
    };
    result.ok_or(HostError::ForbiddenMount)
}

fn sha256_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|digest| lower_hex(digest, 64))
}

fn lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
