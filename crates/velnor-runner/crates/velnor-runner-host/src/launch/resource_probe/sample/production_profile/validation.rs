// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::launch::resource_probe::provider::ProbeImageOrigin;
use crate::launch::verified_images::{WorkerImageOrigin, WorkerImagesOrigin};

pub(super) fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn digest_reference(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(digest)
}

pub(super) fn source_revision(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn complete_worker_image(image: &crate::launch::VerifiedImageIdentity) -> bool {
    digest_reference(image.runtime_id())
        && match image.origin() {
            WorkerImageOrigin::Registry(registry) => {
                digest_reference(&registry.identity.registry_digest)
                    && registry.observation.manifest_size() > 0
                    && digest_reference(&registry.identity.config_digest)
                    && registry.observation.validate(&registry.identity).is_ok()
            }
            WorkerImageOrigin::Developer => true,
        }
}

pub(super) fn matching_origins(probe: &ProbeImageOrigin, workers: &WorkerImagesOrigin) -> bool {
    match (probe, workers) {
        (
            ProbeImageOrigin::Registry {
                manifest_sha256: probe,
            },
            WorkerImagesOrigin::Registry {
                manifest_sha256: worker,
            },
        ) => digest(probe) && probe == worker,
        (ProbeImageOrigin::Developer, WorkerImagesOrigin::Developer) => true,
        _ => false,
    }
}
