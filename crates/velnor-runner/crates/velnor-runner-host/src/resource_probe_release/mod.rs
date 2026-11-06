// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use crate::error::HostError;

mod files;
mod manifest;
pub(crate) mod runner_manifest;
mod runner_qualification;
mod schema5;

fn is_exact_semver(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !(part.is_empty() || part.len() > 1 && part.starts_with('0'))
                && part.bytes().all(|byte| byte.is_ascii_digit())
        })
}

pub fn validate_smoke(output: &Path, root: &Path) -> Result<(), HostError> {
    schema5::validate(output, root)
}

pub fn write_manifest(inspect: &Path, archive: &Path, output: &Path) -> Result<(), HostError> {
    manifest::write(inspect, archive, output)
}

pub fn write_runner_image_manifest(
    descriptors: &Path,
    runner_lock: &Path,
    output: &Path,
) -> Result<(), HostError> {
    runner_manifest::write(descriptors, runner_lock, output)
}

pub async fn qualify_runner_images(
    root: &Path,
    source_sha: &str,
    runner_id: &str,
    dind_id: &str,
    docker_socket: &str,
) -> Result<(), HostError> {
    runner_qualification::qualify(root, source_sha, runner_id, dind_id, docker_socket).await
}
