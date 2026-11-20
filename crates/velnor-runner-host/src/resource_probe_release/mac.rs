// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{files, runner_manifest};
use crate::{error::HostError, launch};
use sha2::{Digest, Sha256};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use velnor_runner_image_manifest::{
    ImageEntry,
    mac_qualification::{
        MAC_QUALIFICATION_SCHEMA, MacDaemonIdentity, MacQualificationImage, MacQualificationImages,
        MacQualificationVersions, MacRunnerQualificationReceipt, parse_mac_runner_qualification,
    },
};

#[derive(Debug, Clone)]
pub struct MacRunnerQualificationRequest {
    pub manifest: PathBuf,
    pub runner_lock: PathBuf,
    pub source_commit: String,
    pub run_id: u64,
    pub run_attempt: u32,
    pub publisher_artifact_id: u64,
    pub docker_socket: String,
    pub state_dir: PathBuf,
    pub receipt_output: PathBuf,
}

pub struct MacRunnerQualifier {
    transport: std::sync::Arc<dyn crate::artifact_admission::RegistryTransport>,
}

impl std::fmt::Debug for MacRunnerQualifier {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MacRunnerQualifier")
            .finish_non_exhaustive()
    }
}

impl MacRunnerQualifier {
    #[must_use]
    pub fn new(
        transport: std::sync::Arc<dyn crate::artifact_admission::RegistryTransport>,
    ) -> Self {
        Self { transport }
    }

    pub fn anonymous() -> Result<Self, HostError> {
        Ok(Self::new(
            crate::artifact_admission::anonymous_registry_transport()?,
        ))
    }

    pub async fn qualify(self, request: MacRunnerQualificationRequest) -> Result<(), HostError> {
        tokio::spawn(async move { qualify(request, self.transport).await })
            .await
            .map_err(|_| HostError::Docker)?
    }
}

async fn qualify(
    request: MacRunnerQualificationRequest,
    transport: std::sync::Arc<dyn crate::artifact_admission::RegistryTransport>,
) -> Result<(), HostError> {
    let end = Instant::now() + Duration::from_mins(80);
    if request.run_id == 0
        || request.run_attempt == 0
        || request.publisher_artifact_id == 0
        || !request.state_dir.is_absolute()
    {
        return Err(HostError::Identity);
    }
    let manifest_bytes = files::read_bounded_regular(&request.manifest, 64 * 1024)?;
    let lock_bytes = files::read_bounded_regular(&request.runner_lock, 64 * 1024)?;
    let manifest =
        runner_manifest::parse_for_source(&manifest_bytes, &request.source_commit, &lock_bytes)?;
    let manifest_sha = hex(&Sha256::digest(&manifest_bytes));
    let versions = MacQualificationVersions {
        runner: manifest.versions.runner.clone(),
        docker: manifest.versions.docker.clone(),
        buildx: manifest.versions.buildx.clone(),
        compose: manifest.versions.compose.clone(),
        node: manifest.versions.node.clone(),
    };
    let runner = manifest.images.runner.clone();
    let dind = manifest.images.dind.clone();
    let probe = manifest.images.resource_probe.clone();
    let observation = launch::qualify_mac_family(
        manifest,
        request.source_commit.clone(),
        manifest_sha.clone(),
        request.docker_socket,
        request.state_dir.join("runner-family-qualification.json"),
        transport,
        end,
    )
    .await?;
    let receipt = MacRunnerQualificationReceipt {
        schema: MAC_QUALIFICATION_SCHEMA.to_owned(),
        source_commit: request.source_commit,
        run_id: request.run_id,
        run_attempt: request.run_attempt,
        publisher_artifact_id: request.publisher_artifact_id,
        manifest_sha256: manifest_sha,
        runner_lock_sha256: hex(&Sha256::digest(&lock_bytes)),
        platform: "linux/amd64".to_owned(),
        images: MacQualificationImages {
            runner: image(runner, observation.runner_id),
            dind: image(dind, observation.dind_id),
            resource_probe: image(probe, observation.probe_id),
        },
        outer_daemon: MacDaemonIdentity {
            id: observation.qualification.outer.id,
            root: observation.qualification.outer.root,
            store: observation.store,
        },
        inner_daemon_id: observation.qualification.inner_id,
        versions,
        cleanup_complete: true,
    };
    let mut bytes = serde_json::to_vec(&receipt).map_err(|_| HostError::Identity)?;
    bytes.push(b'\n');
    parse_mac_runner_qualification(&bytes).map_err(|_| HostError::Identity)?;
    files::create_new_regular(&request.receipt_output, &bytes)
}

fn image(entry: ImageEntry, local_id: String) -> MacQualificationImage {
    MacQualificationImage {
        repository: entry.repository,
        registry_digest: entry.registry_digest,
        platform_manifest_digest: entry.platform_manifest_digest,
        config_digest: entry.config_digest,
        local_id,
    }
}

fn hex(bytes: &[u8]) -> String {
    {
        const ALPHABET: &[u8; 16] = b"0123456789abcdef";
        bytes
            .iter()
            .flat_map(|byte| {
                [
                    char::from(ALPHABET[usize::from(byte >> 4)]),
                    char::from(ALPHABET[usize::from(byte & 15)]),
                ]
            })
            .collect()
    }
}
