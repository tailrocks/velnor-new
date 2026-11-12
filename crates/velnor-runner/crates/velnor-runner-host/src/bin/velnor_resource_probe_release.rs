// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![allow(
    missing_docs,
    reason = "documentation comments are intentionally absent from this codebase"
)]
#![allow(
    clippy::missing_errors_doc,
    reason = "documentation comments are intentionally absent from this codebase"
)]
use std::ffi::OsString;
use std::path::PathBuf;

use velnor_runner_host::{HostError, resource_probe_release};

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("resource probe release: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), HostError> {
    let mut arguments = std::env::args_os().skip(1);
    let command = arguments.next().ok_or(HostError::Identity)?;
    let arguments = arguments.collect::<Vec<OsString>>();
    match (command.to_str(), arguments.as_slice()) {
        (Some("validate-smoke"), [output, root]) => resource_probe_release::validate_smoke(
            &PathBuf::from(output.as_os_str()),
            &PathBuf::from(root.as_os_str()),
        ),
        (Some("write-manifest"), [inspect, archive, output]) => {
            resource_probe_release::write_manifest(
                &PathBuf::from(inspect.as_os_str()),
                &PathBuf::from(archive.as_os_str()),
                &PathBuf::from(output.as_os_str()),
            )
        }
        (Some("qualify-runner-images"), [root, source_sha, runner_id, dind_id, docker_socket]) => {
            let root = PathBuf::from(root);
            let source_sha = source_sha.to_str().ok_or(HostError::Identity)?;
            let runner_id = runner_id.to_str().ok_or(HostError::Identity)?;
            let dind_id = dind_id.to_str().ok_or(HostError::Identity)?;
            let docker_socket = docker_socket.to_str().ok_or(HostError::Identity)?;
            resource_probe_release::qualify_runner_images(
                &root,
                source_sha,
                runner_id,
                dind_id,
                docker_socket,
            )
            .await
        }
        (Some("qualify-runner-image-archives"), values) => qualify_archives(values).await,
        (Some("check-runner-image-candidate"), values) => check_candidate(values).await,
        _ => Err(HostError::Identity),
    }
}

fn argument(value: &OsString) -> Result<&str, HostError> {
    value.to_str().ok_or(HostError::Identity)
}

async fn check_candidate(values: &[OsString]) -> Result<(), HostError> {
    let [
        root,
        source_sha,
        runner,
        dind,
        probe,
        runner_envelope,
        dind_envelope,
        probe_envelope,
        contexts,
        docker_socket,
    ] = values
    else {
        return Err(HostError::Identity);
    };
    let request = resource_probe_release::RunnerCandidateCheck {
        root: std::path::Path::new(root),
        source_sha: argument(source_sha)?,
        runner_archive: std::path::Path::new(runner),
        dind_archive: std::path::Path::new(dind),
        probe_archive: std::path::Path::new(probe),
        runner_envelope: std::path::Path::new(runner_envelope),
        dind_envelope: std::path::Path::new(dind_envelope),
        probe_envelope: std::path::Path::new(probe_envelope),
        contexts: std::path::Path::new(contexts),
        docker_socket: argument(docker_socket)?,
    };
    resource_probe_release::check_runner_image_candidate(&request).await
}

async fn qualify_archives(values: &[OsString]) -> Result<(), HostError> {
    let [
        root,
        source_sha,
        runner_archive,
        dind_archive,
        probe_archive,
        runner_envelope,
        dind_envelope,
        probe_envelope,
        contexts,
        runner_record,
        dind_record,
        probe_record,
        producer_run_id,
        producer_run_attempt,
        qualifier_run_id,
        qualifier_run_attempt,
        qualifier_job_id,
        docker_socket,
        output,
    ] = values
    else {
        return Err(HostError::Identity);
    };
    let request = resource_probe_release::RunnerArchiveQualification {
        root: std::path::Path::new(root),
        source_sha: argument(source_sha)?,
        runner_archive: std::path::Path::new(runner_archive),
        dind_archive: std::path::Path::new(dind_archive),
        probe_archive: std::path::Path::new(probe_archive),
        runner_envelope: std::path::Path::new(runner_envelope),
        dind_envelope: std::path::Path::new(dind_envelope),
        probe_envelope: std::path::Path::new(probe_envelope),
        contexts: std::path::Path::new(contexts),
        runner_record: std::path::Path::new(runner_record),
        dind_record: std::path::Path::new(dind_record),
        probe_record: std::path::Path::new(probe_record),
        producer_run_id: argument(producer_run_id)?
            .parse()
            .map_err(|_| HostError::Identity)?,
        producer_run_attempt: argument(producer_run_attempt)?
            .parse()
            .map_err(|_| HostError::Identity)?,
        qualifier_run_id: argument(qualifier_run_id)?
            .parse()
            .map_err(|_| HostError::Identity)?,
        qualifier_run_attempt: argument(qualifier_run_attempt)?
            .parse()
            .map_err(|_| HostError::Identity)?,
        qualifier_job_id: argument(qualifier_job_id)?
            .parse()
            .map_err(|_| HostError::Identity)?,
        docker_socket: argument(docker_socket)?,
        output: std::path::Path::new(output),
    };
    resource_probe_release::qualify_runner_image_archives(&request).await
}
