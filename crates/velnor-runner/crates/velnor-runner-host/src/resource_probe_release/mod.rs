// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use crate::error::HostError;

mod files;
mod manifest;
pub(crate) mod runner_manifest;
pub(crate) mod runner_qualification;
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

pub async fn qualify_runner_images(
    root: &Path,
    source_sha: &str,
    runner_id: &str,
    dind_id: &str,
    docker_socket: &str,
) -> Result<(), HostError> {
    Box::pin(runner_qualification::qualify(
        root,
        source_sha,
        runner_id,
        dind_id,
        docker_socket,
    ))
    .await
}

#[derive(Debug)]
pub struct RunnerArchiveQualification<'a> {
    pub root: &'a Path,
    pub source_sha: &'a str,
    pub runner_archive: &'a Path,
    pub dind_archive: &'a Path,
    pub probe_archive: &'a Path,
    pub runner_envelope: &'a Path,
    pub dind_envelope: &'a Path,
    pub probe_envelope: &'a Path,
    pub contexts: &'a Path,
    pub runner_record: &'a Path,
    pub dind_record: &'a Path,
    pub probe_record: &'a Path,
    pub producer_run_id: u64,
    pub producer_run_attempt: u32,
    pub qualifier_run_id: u64,
    pub qualifier_run_attempt: u32,
    pub qualifier_job_id: u64,
    pub docker_socket: &'a str,
    pub output: &'a Path,
}

pub async fn qualify_runner_image_archives(
    request: &RunnerArchiveQualification<'_>,
) -> Result<(), HostError> {
    Box::pin(runner_qualification::qualify_archives(request)).await
}

#[derive(Debug)]
pub struct RunnerCandidateCheck<'a> {
    pub root: &'a Path,
    pub source_sha: &'a str,
    pub runner_archive: &'a Path,
    pub dind_archive: &'a Path,
    pub probe_archive: &'a Path,
    pub runner_envelope: &'a Path,
    pub dind_envelope: &'a Path,
    pub probe_envelope: &'a Path,
    pub contexts: &'a Path,
    pub docker_socket: &'a str,
}

pub async fn check_runner_image_candidate(
    request: &RunnerCandidateCheck<'_>,
) -> Result<(), HostError> {
    Box::pin(runner_qualification::check_candidate(request)).await
}
