// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use crate::error::HostError;

mod files;
mod manifest;
pub(crate) mod runner_manifest;
mod runner_qualification;
mod smoke;

fn is_exact_semver(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !(part.is_empty() || part.len() > 1 && part.starts_with('0'))
                && part.bytes().all(|byte| byte.is_ascii_digit())
        })
}

pub fn validate_smoke(output: &Path, root: &Path) -> Result<(), HostError> {
    smoke::validate(output, root)
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

pub fn validate_runner_image_manifest(
    bytes: &[u8],
    source_commit: &str,
    runner_lock: &[u8],
) -> Result<(), HostError> {
    runner_manifest::parse_for_source(bytes, source_commit, runner_lock).map(|_| ())
}

pub async fn qualify_runner_images(
    root: &Path,
    source_sha: &str,
    runner_id: &str,
    dind_id: &str,
    probe_id: &str,
    docker_socket: &str,
    result_path: &Path,
) -> Result<(), HostError> {
    runner_qualification::qualify(
        root,
        source_sha,
        runner_id,
        dind_id,
        probe_id,
        docker_socket,
        result_path,
    )
    .await
}
