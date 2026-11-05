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
        (Some("write-runner-image-manifest"), [descriptors, runner_lock, output]) => {
            resource_probe_release::write_runner_image_manifest(
                &PathBuf::from(descriptors.as_os_str()),
                &PathBuf::from(runner_lock.as_os_str()),
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
        _ => Err(HostError::Identity),
    }
}
